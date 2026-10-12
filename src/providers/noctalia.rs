use crate::providers::background;
use crate::providers::noctalia_runtime::{
    noctalia_config_dir, noctalia_msg, noctalia_state_dir, plugin_enabled_probe, MPVPAPER_PLUGIN_ID,
};
use crate::providers::wallpaper_authority::{self, SavePlan, WallpaperKind};
use crate::theme_manager::{
    DeletableArtifact, PreviewRole, PreviewSource, PreviewSourceKind, ProviderCapabilities,
    ThemeFact, ThemeProvider, ThemeVideoUrl,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Remove a stale background artifact, logging failures instead of
/// swallowing them: a silent failure leaves the wrong background record
/// behind and the next apply resurrects it. A missing file is the normal
/// case (nothing stale) and stays quiet.
fn remove_stale_artifact(path: &Path) {
    if let Err(e) = fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "[noctalia-v5] Could not remove stale {}: {}",
                path.display(),
                e
            );
        }
    }
}

/// Read the `noctalia/mpvpaper` enabled state from a settings.toml text.
///
/// D4: applying a theme restores settings.toml verbatim and runs
/// `config-reload`, which re-enables a plugin the user disabled (every
/// saved theme carries it enabled while the keeper runs disabled). The
/// `[plugins] enabled = [...]` list is machine state, not theme content,
/// so only that section counts: other sections, commented lines and
/// trailing comments never match, and a multi-line array is followed to
/// its closing bracket. `false` for anything unparseable — the caller
/// then treats the state as unknown, never as enabled.
fn mpvpaper_enabled_in_settings(raw: &str) -> bool {
    let mut in_plugins = false;
    let mut collecting = false;
    let mut depth: i32 = 0;
    let mut buf = String::new();
    let mut found = false;
    for line in raw.lines() {
        let stripped = line.trim();
        if stripped.starts_with('#') || stripped.is_empty() {
            continue;
        }
        // Machine-written file: no '#' inside quoted values, so a '#'
        // always starts a trailing comment.
        let code = stripped.split('#').next().unwrap_or("").trim();
        if let Some(header) = code.strip_prefix('[') {
            let header = header.split(']').next().unwrap_or("").trim();
            in_plugins = header == "plugins";
            collecting = false;
            continue;
        }
        if !in_plugins {
            continue;
        }
        let piece = if !collecting {
            match code.strip_prefix("enabled") {
                Some(rest) => {
                    let rest = rest.trim_start();
                    match rest.strip_prefix('=') {
                        Some(value) => value,
                        None => continue,
                    }
                }
                None => continue,
            }
        } else {
            code
        };
        if !collecting {
            buf.clear();
            collecting = true;
        }
        buf.push_str(piece);
        depth += piece.chars().filter(|c| *c == '[').count() as i32
            - piece.chars().filter(|c| *c == ']').count() as i32;
        if depth <= 0 {
            collecting = false;
            if buf.contains("\"noctalia/mpvpaper\"") || buf.contains("'noctalia/mpvpaper'") {
                found = true;
            }
            buf.clear();
        }
    }
    found
}

/// Tri-state variant of [`mpvpaper_enabled_in_settings`] for the CAPTURE
/// gate: `Some(true)`/`Some(false)` only when an explicit `[plugins]
/// enabled` entry was actually located and parsed; `None` when the section,
/// the key or the value cannot be found (missing, other section, or a
/// future format drift). The capture arm reads `None` as unknown — never
/// as disabled — and resolves it through the IPC probe.
///
/// `mpvpaper_enabled_in_settings` keeps its false-for-unparseable contract:
/// `apply()`'s re-assert must read that as "leave the reloaded state
/// alone", where folding an unreadable file into "disabled" is the safe
/// reading. The capture arm needs the opposite default, so it gets its own
/// reader instead of overloading the apply one.
fn mpvpaper_state_in_settings(raw: &str) -> Option<bool> {
    let mut in_plugins = false;
    let mut collecting = false;
    let mut depth: i32 = 0;
    let mut buf = String::new();
    let mut saw_enabled = false;
    let mut found = false;
    for line in raw.lines() {
        let stripped = line.trim();
        if stripped.starts_with('#') || stripped.is_empty() {
            continue;
        }
        // Machine-written file: no '#' inside quoted values, so a '#'
        // always starts a trailing comment.
        let code = stripped.split('#').next().unwrap_or("").trim();
        if let Some(header) = code.strip_prefix('[') {
            let header = header.split(']').next().unwrap_or("").trim();
            in_plugins = header == "plugins";
            collecting = false;
            continue;
        }
        if !in_plugins {
            continue;
        }
        let piece = if !collecting {
            match code.strip_prefix("enabled") {
                Some(rest) => {
                    let rest = rest.trim_start();
                    match rest.strip_prefix('=') {
                        Some(value) => value,
                        None => continue,
                    }
                }
                None => continue,
            }
        } else {
            code
        };
        if !collecting {
            buf.clear();
            collecting = true;
            saw_enabled = true;
        }
        buf.push_str(piece);
        depth += piece.chars().filter(|c| *c == '[').count() as i32
            - piece.chars().filter(|c| *c == ']').count() as i32;
        if depth <= 0 {
            collecting = false;
            if buf.contains("\"noctalia/mpvpaper\"") || buf.contains("'noctalia/mpvpaper'") {
                found = true;
            }
            buf.clear();
        }
    }
    if saw_enabled {
        Some(found)
    } else {
        None
    }
}

/// Flip `[wallpaper.automation] enabled` to `false` in a Noctalia
/// settings.toml text, preserving every other byte.
///
/// Phase 2 of the wallpaper-authority unit: applying a theme restores the
/// whole settings.toml, which re-arms Noctalia's own static rotation
/// (`[wallpaper.automation] enabled = true`, every N seconds). While a
/// video owns the screen that rotation paints a stale image over the
/// poster apply pinned, so the live flag is turned off after the restore.
/// Line-based like [`mpvpaper_enabled_in_settings`] (there is no toml
/// crate): only the `[wallpaper.automation]` section counts, a trailing
/// comment survives, and an already-`false` or missing flag returns `None`
/// so the caller stays idempotent.
fn disable_wallpaper_automation(raw: &str) -> Option<String> {
    let mut in_section = false;
    let mut changed = false;
    let mut out = String::with_capacity(raw.len() + 1);
    for (i, line) in raw.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let stripped = line.trim_start();
        // Machine-written file: a '#' always starts a trailing comment.
        let code = stripped.split('#').next().unwrap_or("").trim();
        if let Some(header) = code.strip_prefix('[') {
            let header = header.split(']').next().unwrap_or("").trim();
            in_section = header == "wallpaper.automation";
            out.push_str(line);
            continue;
        }
        if in_section && !changed {
            if let Some(rest) = code.strip_prefix("enabled") {
                let rest = rest.trim_start();
                if let Some(value) = rest.strip_prefix('=') {
                    if value.trim() == "true" {
                        // Replace the value token in the ORIGINAL line,
                        // keeping indentation and any trailing comment.
                        if let Some(eq_at) = line.find('=') {
                            let after = &line[eq_at + 1..];
                            if let Some(rel) = after.find("true") {
                                let abs = eq_at + 1 + rel;
                                out.push_str(&line[..abs]);
                                out.push_str("false");
                                out.push_str(&line[abs + 4..]);
                                changed = true;
                                continue;
                            }
                        }
                    }
                }
            }
        }
        out.push_str(line);
    }
    if changed {
        Some(out)
    } else {
        None
    }
}

/// Whether a settings.toml text carries a `[wallpaper.automation]` section.
/// Keeps the neutralize step's log honest: a MISSING section is a warning
/// (there is nothing to neutralize), while a section already carrying
/// `enabled = false` is the idempotent no-op and stays quiet.
fn has_wallpaper_automation_section(raw: &str) -> bool {
    raw.lines().any(|line| {
        let stripped = line.trim_start();
        let code = stripped.split('#').next().unwrap_or("").trim();
        code.strip_prefix('[')
            .and_then(|h| h.split(']').next())
            .map(|h| h.trim() == "wallpaper.automation")
            .unwrap_or(false)
    })
}

/// Whether a settings.toml section header names part of the Noctalia
/// LAYOUT — the structure the keeper SEES on screen, as opposed to machine
/// or behaviour state.
///
/// U1 of `odd/tasks/noctalia-layout-survives-the-apply.md`: the roots that
/// count are `[bar]` (plus its per-bar tables `[bar.<name>]`, which carry
/// the bar widgets), `[desktop_widgets]`, `[dock]`, `[shell]` and the
/// per-widget `[widget.<name>]` tables. Deliberately NOT layout:
/// `[plugins]` (machine state, owned by the D4 re-assert), `[hooks]`
/// (behaviour the keeper's watchdog heals), `[wallpaper*]`, `[osd*]` and
/// everything else — those may legitimately change under a live apply, so
/// demanding them would turn this guard into the textual equality the
/// contract forbids.
fn is_layout_section(header: &str) -> bool {
    ["bar", "desktop_widgets", "dock", "shell", "widget"]
        .iter()
        .any(|root| {
            header == *root
                || header
                    .strip_prefix(root)
                    .is_some_and(|rest| rest.starts_with('.'))
        })
}

/// The section name inside a settings.toml header line's CODE (comment
/// already stripped): `[bar]` -> `bar`, `[[bar.x.capsule_group]]` ->
/// `bar.x.capsule_group`, surrounding quotes trimmed. `None` when the
/// line is not a header. Handles the `[[array-of-tables]]` spelling the
/// refutation measured in Noctalia's own re-persist — the old parser
/// read it as a name that never matched layout and let it terminate the
/// enclosing section (LOW of the refutation).
fn section_header_name(code: &str) -> Option<String> {
    let inner = code.strip_prefix('[')?;
    let inner = inner.strip_prefix('[').unwrap_or(inner);
    let name = inner
        .split(']')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(|c| c == '"' || c == '\'');
    Some(name.to_string())
}

/// The layout section NAMES a settings.toml text declares — only the
/// headers [`is_layout_section`] accepts, in file order, each name once.
/// Line-based, like every other settings.toml reader in this module (there
/// is no toml crate): blank lines and comments are skipped (`#` always
/// starts one in this machine-written file), indentation is ignored, and
/// both `[section]` and `[[array-of-tables]]` headers parse.
fn layout_sections(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }
        let code = stripped.split('#').next().unwrap_or("").trim();
        if let Some(name) = section_header_name(code) {
            if is_layout_section(&name) && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// U1 + U5 of `odd/tasks/noctalia-layout-survives-the-apply.md`: is the
/// layout the theme's settings.toml declares still in force in the live
/// one? Pure — two texts in, the missing sections out (empty vec = in
/// force).
///
/// THE RULE: layout is in force iff every layout SECTION the theme
/// declares is present in the live text. Headers only — indentation,
/// comments, quotes and `[[array-of-tables]]` spellings all parse; keys
/// and VALUES are never compared. What Noctalia's re-persist destroys is
/// whole sections: the keeper's failing apply dropped `[hooks]` and then
/// `[bar]` ENTIRELY, and the ratchet kept it gone.
///
/// THE LIMIT, stated: keys INSIDE a surviving section are deliberately
/// invisible, because Noctalia migrates its own schema under sections
/// that stay present — the measured healthy JoKer pair (the packaged
/// theme vs its live re-persist, `THEME_MIGRATED` / `LIVE_MIGRATED` in
/// the tests) differs by five key renames while the section set is
/// identical. A section that survives but lost declared keys therefore
/// reads as in force; the guard only catches the whole-section loss the
/// ratchet actually produces. A theme declaring no layout is vacuously
/// in force (there is nothing to enforce).
///
/// THE SECOND LIMIT, stated (U11): losing ONE of several entries of a
/// repeated `[[array-of-tables]]` block is likewise INVISIBLE — presence
/// semantics count the TABLE NAME, not its entries, and one surviving
/// entry already declares the name. The repair (U10) restores every
/// entry only when the whole table is missing; a live file that kept one
/// entry of a multi-entry table reads as in force even if the theme
/// declared two. Stated limit, by design: entry loss is not what the
/// ratchet produces (it drops whole sections), and counting entries
/// would move the predicate toward the textual equality the contract
/// forbids.
///
/// The report names each missing section once, whole: `[section]`.
fn missing_theme_layout(theme_settings: &str, live_settings: &str) -> Vec<String> {
    let live = layout_sections(live_settings);
    layout_sections(theme_settings)
        .into_iter()
        .filter(|name| !live.contains(name))
        .map(|name| format!("[{name}]"))
        .collect()
}

/// U6 of `odd/tasks/noctalia-layout-survives-the-apply.md`, corrected by
/// U10: re-add ONLY the layout sections missing from the live text —
/// copied verbatim from the theme, in theme order, EVERY entry of each —
/// appended at the end. Every other byte of the live text is left alone.
/// Pure, so both repair invariants are pinned on texts. `None` when
/// nothing is missing (the caller then performs no rewrite at all).
///
/// THE TWO INVARIANTS (HIGH-2 of the refutation; the refuted repair
/// copied the theme's settings.toml VERBATIM, undoing both):
/// - it must never restore the theme's `[plugins]` list — a verbatim
///   copy + reload re-enables a plugin the keeper turned off, the very
///   operation D4 exists to forbid;
/// - it must never revert the palette / wallpaper state the apply (and
///   Noctalia's own re-persist of it) wrote after step 0's copy.
///
/// EVERY ENTRY of a repeated block (U10, verified against the keeper's
/// real files): a `[[array-of-tables]]` header may repeat — each
/// occurrence is ONE ARRAY ENTRY the theme declares (JoKer, Night and
/// Work carry two `[[bar.barra_lateral.capsule_group]]` entries while
/// `center = [ "group:g1", "ram", "group:g2" ]` references them by id).
/// The refuted repair restored only the FIRST entry of each repeated
/// block, so the bar came back missing `group:g2` with the reference
/// dangling — and the presence-based check then reported `[]`, letting
/// the apply log success on a HALF-restored layout. Restoring every
/// occurrence closes that: whatever the repair recognised as missing is
/// fully back before it writes. A plain `[section]` occurs once in valid
/// TOML, so nothing changes for it.
///
/// Appending whole section blocks is safe TOML: a missing `[bar]` may be
/// defined after its surviving `[bar.<name>]` sub-tables (defining a
/// super-table later is legal as long as it was never defined), and a
/// block carries its header, keys, comments and `[[array-of-tables]]`
/// bodies verbatim up to the next header.
fn restore_missing_layout_sections(theme_settings: &str, live_settings: &str) -> Option<String> {
    let live = layout_sections(live_settings);
    let missing: Vec<String> = layout_sections(theme_settings)
        .into_iter()
        .filter(|name| !live.contains(name))
        .collect();
    if missing.is_empty() {
        return None;
    }
    // Every block whose header names a missing section, in theme order:
    // the header line plus every line up to (excluding) the next header.
    // Repeats are NOT deduplicated — see the U10 note above.
    let mut blocks: Vec<Vec<String>> = Vec::new();
    let mut capture: Option<usize> = None;
    for line in theme_settings.split('\n') {
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            if let Some(i) = capture {
                blocks[i].push(line.to_string());
            }
            continue;
        }
        let code = stripped.split('#').next().unwrap_or("").trim();
        if let Some(name) = section_header_name(code) {
            capture = if missing.contains(&name) {
                blocks.push(vec![line.to_string()]);
                Some(blocks.len() - 1)
            } else {
                None
            };
            continue;
        }
        if let Some(i) = capture {
            blocks[i].push(line.to_string());
        }
    }
    let mut out = String::with_capacity(theme_settings.len() + live_settings.len());
    out.push_str(live_settings);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        // One blank line separates the untouched live text from the
        // re-added blocks.
        out.push('\n');
    }
    for block in &blocks {
        for line in block {
            out.push_str(line);
            out.push('\n');
        }
    }
    Some(out)
}

/// What [`sanitize_settings_toml`] did to a settings.toml text.
///
/// `rewritten` counts the absolute values that pointed INSIDE the theme and
/// were turned into theme-relative ones (the packaged asset stays reachable).
/// `dropped` counts the machine-only absolute values whose whole line went
/// away, and `dropped_keys` names them in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SanitizeReport {
    /// How many in-theme absolute values became theme-relative.
    pub rewritten: usize,
    /// How many machine-only lines were dropped.
    pub dropped: usize,
    /// The key of every dropped line, in file order.
    pub dropped_keys: Vec<String>,
}

impl SanitizeReport {
    /// Whether the sanitizer left the text byte-identical.
    pub fn is_empty(&self) -> bool {
        self.rewritten == 0 && self.dropped == 0
    }
}

/// The `key` of a `key = value` line and the byte range of the value's
/// CONTENT inside that line (quotes excluded), when the value is a single
/// token.
struct ValueToken<'a> {
    key: &'a str,
    value: &'a str,
    content: std::ops::Range<usize>,
}

/// Read one settings.toml line as `key = value`, when the value is a single
/// token: a quoted string or a bare word. Anything else — a section header, a
/// comment, a blank line, a line with no `=`, or a compound value such as an
/// array — is `None`, so the caller leaves it alone.
///
/// Machine-written file: no `#` inside quoted values, so a `#` always starts a
/// trailing comment (the same convention as [`mpvpaper_enabled_in_settings`]).
fn keyed_value_token(line: &str) -> Option<ValueToken<'_>> {
    let code = line.split('#').next().unwrap_or("").trim_end();
    if code.trim().is_empty() {
        return None;
    }
    let eq_at = code.find('=')?;
    let key = code[..eq_at].trim();
    if key.is_empty() {
        return None;
    }
    // `code` is a prefix of `line`, so `eq_at` is valid in both.
    let after = &line[eq_at + 1..];
    let value_at = eq_at + 1 + (after.len() - after.trim_start().len());
    let rest = &line[value_at..];
    match rest.chars().next()? {
        quote @ ('"' | '\'') => {
            let end = rest[1..].find(quote)? + 1;
            Some(ValueToken {
                key,
                value: &rest[1..end],
                content: value_at + 1..value_at + end,
            })
        }
        '[' => None, // an array is theme content, never one path
        _ => {
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '#')
                .unwrap_or(rest.len());
            if end == 0 {
                return None;
            }
            Some(ValueToken {
                key,
                value: &rest[..end],
                content: value_at..value_at + end,
            })
        }
    }
}

/// Append `line` to `out`, keeping the original line structure (`split('\n')`
/// + a separator before every line but the first).
fn push_line(out: &mut String, first: &mut bool, line: &str) {
    if !*first {
        out.push('\n');
    }
    *first = false;
    out.push_str(line);
}

/// Remove every path that belongs to the machine that saved the theme from a
/// Noctalia `settings.toml` text, keeping every other byte identical.
///
/// Line-based like [`mpvpaper_enabled_in_settings`] (there is no toml crate):
/// a `key = value` line whose value is an absolute path is either rewritten or
/// dropped.
///
/// - An absolute value INSIDE `theme_dir` is rewritten to its theme-relative
///   form, so a packaged asset the theme carries stays reachable.
/// - Any other absolute value is machine-only: the whole line goes. Emptying
///   the value or guessing a replacement would be a lie.
/// - Every other line — relative values, non-path values, arrays, comments,
///   blank lines, section headers — is returned byte-for-byte.
pub fn sanitize_settings_toml(text: &str, theme_dir: &Path) -> (String, SanitizeReport) {
    let mut report = SanitizeReport::default();
    let mut out = String::with_capacity(text.len());
    let mut first = true;
    for line in text.split('\n') {
        if let Some(token) = keyed_value_token(line) {
            let value = Path::new(token.value);
            if value.is_absolute() {
                match value.strip_prefix(theme_dir) {
                    // The value names an asset the theme itself carries: keep
                    // it reachable as a theme-relative path.
                    Ok(relative) if !relative.as_os_str().is_empty() => {
                        let mut rewritten = String::with_capacity(line.len());
                        rewritten.push_str(&line[..token.content.start]);
                        rewritten.push_str(&relative.display().to_string());
                        rewritten.push_str(&line[token.content.end..]);
                        report.rewritten += 1;
                        push_line(&mut out, &mut first, &rewritten);
                        continue;
                    }
                    // Any other absolute value — the theme dir itself
                    // included — is machine-only: the whole line goes.
                    _ => {
                        report.dropped += 1;
                        report.dropped_keys.push(token.key.to_string());
                        continue;
                    }
                }
            }
        }
        push_line(&mut out, &mut first, line);
    }
    (out, report)
}

/// Drop the theme's absolute background records (`wallpaper.txt`,
/// `video.txt`).
///
/// They name the machine that saved the theme, so they must not travel: the
/// packaged equivalent (`poster.txt` / `video-media.txt`) is what a target
/// machine resolves. A record with NO packaged equivalent still goes — a
/// foreign absolute path is the defect being removed — and that loss is
/// reported instead of hidden.
pub fn drop_absolute_background_records(
    provider_dir: &Path,
    has_packaged_poster: bool,
    has_packaged_video: bool,
) {
    let static_record = provider_dir.join("wallpaper.txt");
    if static_record.exists() {
        remove_stale_artifact(&static_record);
        if has_packaged_poster {
            tracing::info!(
                "[noctalia-v5] Dropped the absolute wallpaper record {} — the packaged poster.txt travels instead",
                static_record.display()
            );
        } else {
            tracing::warn!(
                "[noctalia-v5] Dropped the absolute wallpaper record {} with no packaged poster to replace it",
                static_record.display()
            );
        }
    }
    let video_record = provider_dir.join(wallpaper_authority::PAINTER_VIDEO_FILE);
    if video_record.exists() {
        remove_stale_artifact(&video_record);
        if has_packaged_video {
            tracing::info!(
                "[noctalia-v5] Dropped the absolute video record {} — the packaged video-media.txt travels instead",
                video_record.display()
            );
        } else {
            tracing::warn!(
                "[noctalia-v5] Dropped the absolute video record {} with no packaged video to replace it",
                video_record.display()
            );
        }
    }
}

/// Sanitize a `settings.toml` that lives inside a theme, in place.
///
/// The theme must be clean from the moment it is saved or packaged: a
/// machine-only path that reaches disk is one nobody looks at again until a
/// foreign machine restores it. Best-effort — an unreadable or unwritable file
/// only warns; the save or the package still succeeds.
pub(crate) fn sanitize_settings_file(dest: &Path, theme_dir: &Path) {
    let raw = match fs::read_to_string(dest) {
        Ok(raw) => raw,
        Err(e) => {
            tracing::warn!(
                "[noctalia-v5] Cannot read {} to sanitize it: {}",
                dest.display(),
                e
            );
            return;
        }
    };
    let (clean, report) = sanitize_settings_toml(&raw, theme_dir);
    if report.is_empty() {
        tracing::debug!(
            "[noctalia-v5] {} carries no machine-only path",
            dest.display()
        );
        return;
    }
    if let Err(e) = fs::write(dest, &clean) {
        tracing::warn!(
            "[noctalia-v5] Cannot write the sanitized {}: {}",
            dest.display(),
            e
        );
        return;
    }
    if report.rewritten > 0 {
        tracing::info!(
            "[noctalia-v5] Rewrote {} settings.toml path(s) to theme-relative in {}",
            report.rewritten,
            dest.display()
        );
    }
    if report.dropped > 0 {
        tracing::warn!(
            "[noctalia-v5] Dropped {} machine-only value(s) from {}: {}",
            report.dropped,
            dest.display(),
            report.dropped_keys.join(", ")
        );
    }
}

/// Phase 2 wiring: turn off Noctalia's own static-wallpaper rotation in the
/// LIVE settings.toml after the theme restore re-armed it, then ask
/// Noctalia to reload. Best-effort and idempotent: a missing file, a
/// missing section, or an already-off flag warns and continues — never
/// fails the apply.
fn neutralize_noctalia_wallpaper_rotation() {
    let Some(settings_path) = noctalia_state_dir().map(|d| d.join("settings.toml")) else {
        tracing::warn!(
            "[noctalia-v5] Cannot resolve the Noctalia state dir; wallpaper rotation left as restored"
        );
        return;
    };
    let raw = match fs::read_to_string(&settings_path) {
        Ok(raw) => raw,
        Err(e) => {
            tracing::warn!(
                "[noctalia-v5] Cannot read live settings.toml to neutralize the wallpaper rotation: {}",
                e
            );
            return;
        }
    };
    let Some(edited) = disable_wallpaper_automation(&raw) else {
        if has_wallpaper_automation_section(&raw) {
            tracing::debug!(
                "[noctalia-v5] [wallpaper.automation] already disabled; nothing to neutralize"
            );
        } else {
            tracing::warn!(
                "[noctalia-v5] No [wallpaper.automation] section in the live settings.toml; wallpaper rotation left as restored"
            );
        }
        return;
    };
    if let Err(e) = fs::write(&settings_path, edited) {
        tracing::warn!(
            "[noctalia-v5] Cannot write live settings.toml to neutralize the wallpaper rotation: {}",
            e
        );
        return;
    }
    tracing::info!(
        "[noctalia-v5] Neutralized Noctalia wallpaper rotation ([wallpaper.automation] enabled -> false)"
    );
    if let Err(e) = noctalia_msg(&["msg", "config-reload"]) {
        tracing::warn!(
            "[noctalia-v5] Config reload after neutralizing the wallpaper rotation failed: {}",
            e
        );
    }
}

/// How long the layout verification waits for Noctalia's own re-persist
/// to land before reading the live settings.toml. U2 of
/// `odd/tasks/noctalia-layout-survives-the-apply.md`: the keeper's journal
/// places that rewrite ~1 s after `config-reload` (the wipe had landed by
/// ~2.5 s), so a verify at t+0 would only read the copy HVE just wrote and
/// would certify a layout that is about to disappear. One bounded settle,
/// never a poll loop. `HVE_LAYOUT_SETTLE_MS` overrides the delay for the
/// hermetic tests, which drive the wipe through a stub CLI.
///
/// U9: this settle is paid by the DETACHED verify worker
/// ([`spawn_layout_verify_and_repair`]), never by the UI thread — the
/// apply returns without waiting for it.
const LAYOUT_SETTLE: Duration = Duration::from_millis(1500);

/// The settle delay for this process (see [`LAYOUT_SETTLE`]). An unparseable
/// override falls back to the production delay — a test seam must never be
/// able to shorten the real guard by being mistyped.
fn layout_settle_delay() -> Duration {
    std::env::var("HVE_LAYOUT_SETTLE_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(LAYOUT_SETTLE)
}

/// U12 of `odd/tasks/noctalia-layout-survives-the-apply.md`: the
/// apply-generation token. Every `apply()` increments this counter at its
/// START, and the detached layout-verify worker captures the value of the
/// apply that spawned it. After its settle and BEFORE it writes anything,
/// the worker compares — a superseded worker must never lay ITS theme's
/// layout over a NEWER apply's file: with the keeper's real themes the
/// section sets differ (JoKer vs Space, measured), so a superseded
/// worker would always find "its" sections missing from the newer theme's
/// healthy file and append them — the desktop would render the old
/// theme's bar under the new theme. U9 (the detached worker) recreated
/// the hazard the synchronous version never had: apply and verify used
/// to be serialized on one thread, so a newer apply could not start
/// mid-verify.
///
/// WHEN it is bumped (U16): the bump happens immediately BEFORE an
/// apply's FIRST write to the live settings.toml — not at the apply's
/// first statement — so supersession coincides with TAKING the duty. An
/// apply that fails before touching the live file never supersedes its
/// predecessor's worker, and the predecessor's worker still repairs.
/// From the bump on, the apply owns the duty and hand it to its
/// [`LayoutVerifySpawnDuty`], which spawns the worker on EVERY exit
/// path — Ok or Err — so a failed apply can never strand the live file
/// unchecked.
static APPLY_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Whether `generation` is still the newest apply generation — i.e. no
/// newer `apply()` has started since the worker's apply captured it.
fn layout_generation_is_current(generation: u64) -> bool {
    APPLY_GENERATION.load(Ordering::SeqCst) == generation
}

/// U16 (residue (e) of the contract) — the DUTY of spawning the detached
/// layout-verify worker. Holds the triple (theme's saved settings.toml,
/// live settings.toml, captured apply generation) once the apply has
/// written the live file, and SPAWNS the worker in its `Drop`.
///
/// WHY a Drop guard instead of spawning at the end of the apply: the
/// apply's early returns are `?` on real failure paths (unreadable
/// `wallpaper.txt`, failed templates, ...) and Rust drops locals on
/// them exactly like on the normal exit. U7's shape — arm after step 0,
/// spawn at the end — left every Err path WITHOUT a worker: the live
/// file was written, a later step failed, and no verify ever ran. The
/// duty must survive BOTH directions: every exit path AFTER the duty
/// was taken spawns the verify, and an apply that fails BEFORE it
/// touched the live file (no duty taken, None stays None) never
/// supersedes its predecessor's repair. Same guard idiom the file
/// already uses for the worker-active counter and the colour-authority
/// hold.
struct LayoutVerifySpawnDuty(std::cell::Cell<Option<(PathBuf, PathBuf, u64)>>);

impl Drop for LayoutVerifySpawnDuty {
    fn drop(&mut self) {
        if let Some((saved_settings, live_settings, generation)) = self.0.take() {
            spawn_layout_verify_and_repair(saved_settings, live_settings, generation);
        }
    }
}

/// U13 of `odd/tasks/noctalia-layout-survives-the-apply.md`: publish
/// `text` at `path` through a temporary SIBLING file plus `rename` (the
/// [`crate::theme_presets`] `copy_preset` idiom), never with an in-place
/// truncate-and-write.
///
/// WHY: this path is shared by writers that can run CONCURRENTLY with
/// the detached layout-repair worker — the apply's own settings restore
/// (`fs::copy`), the rotation neutralizer's `fs::write`, and the
/// keeper's watchdog outside HVE. A truncate-and-write in place lets a
/// reader of the same path observe a torn file (half the old bytes,
/// half the new); rename is atomic at the filesystem level, so every
/// concurrent reader sees either the complete old file or the complete
/// new one. What it does NOT cover is registered in the contract (U15):
/// other writers of this path are still in-place, and a process exit
/// after writing the temporary but before the rename leaves the old
/// file intact plus a stale `.part-` sibling (U17: named with THIS
/// invocation's unique suffix, so two workers can never hurt each
/// other's in-flight temporary, and a never-raced stale leftover is
/// traceable to exactly one write attempt).
fn write_settings_atomically(path: &Path, text: &str, generation: u64) -> std::io::Result<()> {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid settings destination {}", path.display()),
        ));
    };
    let tmp = atomic_temp_sibling(path, name, generation);
    let _ = std::fs::remove_file(&tmp);
    if let Err(e) = fs::write(&tmp, text) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Monotonic sequence counter for [`atomic_temp_sibling`] — U17: with
/// pid + generation + sequence, every temporary sibling ever created in
/// this process is a distinct name, so two workers inside the atomic
/// write can never delete or rename each other's in-flight temporary
/// (the duplicate-name hazard the contract's residue (f) describes).
static ATOMIC_WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

/// The temporary sibling for an atomic publish of `path` (the bounded
/// layout repair's only writer): `[ .part-<name>.<pid>-<generation>-<seq> ]`.
/// Unique per process (pid), per apply (generation) and per invocation
/// (monotonic sequence) — U17 (residue (f)).
fn atomic_temp_sibling(path: &Path, name: &str, generation: u64) -> PathBuf {
    let seq = ATOMIC_WRITE_SEQ.fetch_add(1, Ordering::SeqCst);
    path.with_file_name(format!(
        ".part-{name}.{}-{generation}-{seq}",
        std::process::id()
    ))
}

/// U2 of `odd/tasks/noctalia-layout-survives-the-apply.md`, corrected by
/// U6/U7 after the verification refuted the first delivery, and by U9
/// after the keeper felt the correction's regression: after the apply's
/// LAST write/reload (the call site arms it at the end of the apply),
/// verify the theme's LAYOUT is still in force, and repair it exactly
/// once when it is not.
///
/// Noctalia re-persists its IN-MEMORY config on its own IPC-driven
/// changes; when that re-persist drops the layout sections the live file
/// ratchets down and no later apply brings them back (the keeper's failing
/// apply: 11741 B copied, 2471 B left, no `[bar]`). The repair is the SAME
/// SHAPE as the D4 `noctalia/mpvpaper` re-assert applied after the reload
/// and carries the SAME best-effort contract: every failure is a warning,
/// never an apply error, and the rewrite + reload happens AT MOST ONCE —
/// no loop — so a rewrite that does not stick is logged honestly and left
/// for the keeper's live look instead of fighting Noctalia forever.
///
/// The rewrite is SURGICAL (U6, HIGH-2 of the refutation):
/// [`restore_missing_layout_sections`] re-adds only the missing layout
/// sections and leaves every other byte alone, so the repair can neither
/// restore the theme's `[plugins]` list (the D4 protection this module
/// already earned) nor revert the palette / wallpaper state the apply
/// itself wrote. The refuted repair copied the theme's file VERBATIM and
/// did both.
///
/// PLACEMENT (U9 — the regression the keeper felt): this function runs on
/// a DETACHED worker — [`spawn_layout_verify_and_repair`] — after the
/// apply returned, never on the apply's critical path. The predicate, the
/// surgical repair and the bounded-once rule below are unchanged; only
/// WHERE the wait and the work happen moved off the UI thread.
///
/// IDENTITY (U12 — the hole U9 opened): the detached worker carries the
/// GENERATION of the apply that spawned it
/// ([`APPLY_GENERATION`]) and, after its settle and again immediately
/// before its reload, yields when a newer apply has started. Without
/// this, the refutation's timeline replays on the keeper's real themes:
/// apply A ends, worker A arms, the keeper clicks B; worker A then wakes
/// against B's healthy file, finds A's sections "missing" (the section
/// sets differ between his themes — measured), appends A's blocks and
/// reloads — the desktop renders A's bar under B. A superseded worker
/// must never write and never reload.
///
/// COST per apply (honest, U7 + U9): a theme that declares layout pays
/// ONE settle ([`LAYOUT_SETTLE`], 1.5 s) on the WORKER before the read —
/// Noctalia's own re-persist lands ~1 s after the last reload, so a
/// verify at t+0 would only read HVE's own copy. When the layout is
/// actually missing, the repair adds ONE reload and ONE more settle
/// (re-check). None of it blocks the apply or the transition. A theme
/// declaring no layout short-circuits before any of it: zero cost.
fn verify_and_repair_layout(saved_settings: &Path, live_settings: &Path, generation: u64) {
    let theme = match fs::read_to_string(saved_settings) {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!(
                "[noctalia-v5] Cannot read the theme's settings.toml to verify its layout: {}",
                e
            );
            return;
        }
    };
    // A theme that declares no layout has nothing to enforce (U3-c):
    // no settle, no read of the live file, no repair. The short-circuit
    // is pinned by `apply_with_a_layoutless_theme_skips_the_settle` —
    // U14 re-pinned it to measure THIS worker's settle, since U9 moved
    // the settle off the apply thread.
    if layout_sections(&theme).is_empty() {
        tracing::debug!(
            "[noctalia-v5] Theme declares no layout sections; nothing to verify"
        );
        return;
    }
    // One bounded settle for Noctalia's own re-persist — see
    // LAYOUT_SETTLE for why the verify cannot read the live file at t+0.
    std::thread::sleep(layout_settle_delay());
    // U12: the settle slept, so a NEWER apply may have started in the
    // meantime (the keeper clicking the next theme while this worker
    // waited). Its file is now the current one; laying THIS theme's
    // layout over it would render the old theme's bar under the new
    // theme. Yield BEFORE reading, writing or reloading anything.
    if !layout_generation_is_current(generation) {
        tracing::info!(
            "[noctalia-v5] A newer theme apply started while this layout verify \
             settled; yielding WITHOUT writing or reloading (superseded generation {generation})"
        );
        return;
    }
    let live = match fs::read_to_string(live_settings) {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!(
                "[noctalia-v5] Cannot read the live settings.toml to verify the theme layout: {}",
                e
            );
            return;
        }
    };
    let missing = missing_theme_layout(&theme, &live);
    let Some(repaired) = restore_missing_layout_sections(&theme, &live) else {
        tracing::debug!("[noctalia-v5] Theme layout verified in force at the end of the apply");
        return;
    };
    tracing::warn!(
        "[noctalia-v5] Noctalia dropped the theme layout during the apply (missing: {}); \
         re-adding ONLY those sections to the live settings.toml and reloading once",
        missing.join(", ")
    );
    // U13: atomic publish (temporary sibling + rename) — this path is
    // shared with the apply's own copy, the rotation neutralizer and
    // the keeper's watchdog, so an in-place truncate could tear the
    // file under one of them. U17: the sibling name carries this
    // worker's generation plus pid + sequence, so no concurrent write
    // attempt can touch its in-flight temporary.
    if let Err(e) = write_settings_atomically(live_settings, &repaired, generation) {
        tracing::warn!(
            "[noctalia-v5] Cannot repair the live settings.toml layout: {}",
            e
        );
        return;
    }
    // U12, second check: re-verify IMMEDIATE BEFORE the reload — the
    // window between the write and the reload is microseconds wide, but
    // a newer apply landing in it must not receive a reload issued on
    // behalf of the superseded theme. The newer apply runs its own
    // `config-reload`; this one stays quiet.
    if !layout_generation_is_current(generation) {
        tracing::info!(
            "[noctalia-v5] A newer theme apply started before the layout repair \
             could reload; skipping the reload (superseded generation {generation})"
        );
        return;
    }
    if let Err(e) = noctalia_msg(&["msg", "config-reload"]) {
        tracing::warn!(
            "[noctalia-v5] Config reload after the layout repair failed: {}",
            e
        );
        return;
    }
    // The single bounded re-check: log whether the ONE repair held, and
    // stop there either way — never a second repair, never a loop.
    std::thread::sleep(layout_settle_delay());
    // U18 (residue (g) of the contract): the settle slept, so a newer
    // apply may have started during it (the keeper clicking the next
    // theme). Without this re-check the comparison below runs the OLD
    // theme's sections against the NEWER theme's healthy file — and with
    // the real themes' differing section sets that logs the false
    // diagnostic "STILL missing after the single repair" against a
    // healthy file. Log-only after this point, but it is a diagnostic
    // lie; yield with the honest message instead.
    if !layout_generation_is_current(generation) {
        tracing::info!(
            "[noctalia-v5] A newer theme apply started during the post-repair \
             settle; skipping the post-repair re-check (superseded generation \
             {generation})"
        );
        return;
    }
    let after = match fs::read_to_string(live_settings) {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!(
                "[noctalia-v5] Cannot re-read the live settings.toml after the layout repair: {}",
                e
            );
            return;
        }
    };
    let still_missing = missing_theme_layout(&theme, &after);
    if still_missing.is_empty() {
        tracing::info!(
            "[noctalia-v5] Theme layout repaired in the live settings.toml (was missing: {})",
            missing.join(", ")
        );
    } else {
        tracing::warn!(
            "[noctalia-v5] Theme layout STILL missing after the single repair (missing: {}); \
             Noctalia rewrote it again — the apply stops here by design",
            still_missing.join(", ")
        );
    }
}

/// Live count of detached layout-verify workers (U9 of
/// `odd/tasks/noctalia-layout-survives-the-apply.md`).
///
/// WHY it exists: same reason as [`REASSERT_WORKERS_ACTIVE`] — the worker
/// reads PROCESS-GLOBAL state (`PATH` to resolve the `noctalia` binary,
/// `HVE_LAYOUT_SETTLE_MS`, the sandboxed `HOME`) and may write the live
/// settings.toml through its bounded repair. The `LayoutStub` test
/// harness waits on this counter (see `LayoutStub::drop`) before
/// releasing its sandbox, so a worker can never outlive its test.
///
/// Production never reads it; it only guards the test sandbox.
static LAYOUT_VERIFY_WORKERS_ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// Decrements [`LAYOUT_VERIFY_WORKERS_ACTIVE`] when the worker ends,
/// panic or not.
struct LayoutVerifyWorkerGuard;

impl Drop for LayoutVerifyWorkerGuard {
    fn drop(&mut self) {
        LAYOUT_VERIFY_WORKERS_ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Whether every detached layout-verify worker has finished. Test-only:
/// the sandbox harness polls it before restoring its process-global env.
#[cfg(test)]
pub(crate) fn layout_verify_workers_idle() -> bool {
    LAYOUT_VERIFY_WORKERS_ACTIVE.load(Ordering::SeqCst) == 0
}

/// U9 of `odd/tasks/noctalia-layout-survives-the-apply.md` — the
/// regression the keeper felt live ("ahora me parece que algo rasca y
/// antes era todo fluido"): fire-and-forget the layout verify + bounded
/// repair AFTER the apply returned.
///
/// WHY the check cannot block the transition: the apply runs
/// SYNCHRONOUSLY ON THE UI THREAD — the call chain is a Slint callback ->
/// the gallery slot `apply_theme` -> the theme manager `apply` -> this
/// provider's `apply`, with no spawn anywhere on that path.
/// `verify_and_repair_layout` waits for Noctalia's re-persist
/// ([`LAYOUT_SETTLE`], 1.5 s) before its read and again before the
/// post-repair re-check — up to ~3 s of UI FREEZE in the middle of the
/// theme transition (measured from the keeper's own log: the transition's
/// cadence is unchanged at 0.50 + 0.95 + 0.40 = 1.85 s, but the
/// wall-clock from Noctalia's config reload to the visible wallpaper
/// grew by ~1 s; the keeper perceives the freeze as stutter). None of
/// that wait is needed by the apply itself: the check only has to observe
/// the live file AFTER the apply's last write — a few ms after this
/// spawn — so it runs here, following the established idiom of the
/// off-thread colour re-assert (`spawn_custom_scheme_reassert`):
/// detached by design, single-shot, bounded, never touches Slint state,
/// and every failure is a warning — a layout problem must never fail the
/// theme apply.
///
/// U12: the worker captures the APPLY GENERATION at spawn time and yields
/// (no write, no reload) when a newer apply has started — see
/// [`verify_and_repair_layout`] for why a superseded worker must never
/// lay its theme's layout over a newer apply's file.
fn spawn_layout_verify_and_repair(
    saved_settings: PathBuf,
    live_settings: PathBuf,
    generation: u64,
) {
    LAYOUT_VERIFY_WORKERS_ACTIVE.fetch_add(1, Ordering::SeqCst);
    // Same spawn discipline as `spawn_custom_scheme_reassert`: the guard
    // is built in the PARENT and moved into the worker, and the spawn goes
    // through `Builder::spawn` (a `Result`, never a panic), so a spawn
    // that fails drops the closure in the parent, the moved guard
    // decrements, and the counter can never stay at +1 for the rest of
    // the process (which would make every later sandbox wait its full
    // deadline).
    let active = LayoutVerifyWorkerGuard;
    let spawned = std::thread::Builder::new()
        .name("hve-noctalia-layout-verify".to_string())
        .spawn(move || {
            let _active = active;
            verify_and_repair_layout(&saved_settings, &live_settings, generation);
        });
    if let Err(e) = spawned {
        // The closure (and its guard) was dropped above, so the count is
        // already back. Say it out loud: a silent failure here would look
        // exactly like "no verify was ever needed".
        tracing::warn!(
            "[noctalia-v5] could not spawn the layout verify worker: {}",
            e
        );
    }
}

/// What apply must do to `noctalia/mpvpaper` after `config-reload` so the
/// theme cannot change whether the plugin is enabled (D4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginReassert {
    Disable,
    Enable,
    Leave,
}

/// Map the pre-apply snapshot to the post-reload action: a disabled plugin
/// stays disabled (a theme carrying it enabled must not leave it
/// enabled), an enabled one stays enabled, unknown stays untouched.
fn plugin_reassert_action(was_enabled: Option<bool>) -> PluginReassert {
    match was_enabled {
        Some(false) => PluginReassert::Disable,
        Some(true) => PluginReassert::Enable,
        None => PluginReassert::Leave,
    }
}

/// Whether the live mpvpaper manifest may be copied into a theme.
///
/// `wants_manifest` is the legacy capture arm (`SavePlan::Unknown` or a
/// vetoed video) — the only arm that ever considered the manifest. The
/// manifest belongs to the `noctalia/mpvpaper` plugin, which OWNS video
/// wallpapers by launching `mpvpaper`; a provably DISABLED plugin owns
/// nothing, so its live `assignments.json` is stale (it still names the
/// previous video). The gallery ranks a live assignment above the theme's
/// own static wallpaper record, so a copied dead manifest would hijack the
/// theme's preview AND its re-apply. `None` (unreadable) is never
/// "disabled": the legacy capture is preserved.
fn should_capture_video_manifest(wants_manifest: bool, plugin_enabled: Option<bool>) -> bool {
    wants_manifest && plugin_enabled != Some(false)
}

/// Snapshot whether `noctalia/mpvpaper` is enabled RIGHT NOW, before apply
/// overwrites the live settings. Primary signal is the live settings.toml
/// (fast, local, no IPC); fallback is the `plugins list` IPC. `None` when
/// neither answers — the caller then leaves the reloaded plugin state
/// alone and says so, instead of changing what it could not read.
fn live_mpvpaper_plugin_enabled() -> Option<bool> {
    if let Some(state_dir) = noctalia_state_dir() {
        match fs::read_to_string(state_dir.join("settings.toml")) {
            Ok(raw) => return Some(mpvpaper_enabled_in_settings(&raw)),
            Err(e) => tracing::debug!(
                "[noctalia-v5] Cannot read live settings.toml for plugin snapshot: {}",
                e
            ),
        }
    }
    // Same predicate as mpvpaper::mpvpaper_enabled, but tri-state: an IPC
    // failure is "unknown", never "disabled" — the seam's probe carries the
    // failure back verbatim, so this warning keeps its cause.
    match plugin_enabled_probe(MPVPAPER_PLUGIN_ID) {
        Ok(enabled) => Some(enabled),
        Err(e) => {
            tracing::warn!(
                "[noctalia-v5] Cannot snapshot plugin state ({}); leaving reloaded state alone",
                e
            );
            None
        }
    }
}

/// Capture-specific snapshot: may `noctalia/mpvpaper` paint right now?
///
/// [`live_mpvpaper_plugin_enabled`] folds an unparseable live settings.toml
/// into `Some(false)` because `apply()`'s re-assert must read that as "leave
/// the reloaded state alone". The capture gate cannot afford that: a future
/// format drift would silently stop capturing the manifest while the plugin
/// is enabled and painting. So the local read is tri-state
/// ([`mpvpaper_state_in_settings`]); when it cannot name an explicit
/// `[plugins] enabled` entry the snapshot asks the `plugins list` IPC, and
/// only when BOTH fail does it return `None` — which the caller treats as
/// unknown (capture), never as disabled.
fn mpvpaper_can_paint_for_capture() -> Option<bool> {
    if let Some(state_dir) = noctalia_state_dir() {
        match fs::read_to_string(state_dir.join("settings.toml")) {
            Ok(raw) => {
                if let Some(state) = mpvpaper_state_in_settings(&raw) {
                    return Some(state);
                }
                tracing::debug!(
                    "[noctalia-v5] Live settings.toml names no explicit noctalia/mpvpaper state; asking the plugin IPC for the capture gate"
                );
            }
            Err(e) => tracing::debug!(
                "[noctalia-v5] Cannot read live settings.toml for the capture snapshot: {}",
                e
            ),
        }
    }
    match plugin_enabled_probe(MPVPAPER_PLUGIN_ID) {
        Ok(enabled) => Some(enabled),
        Err(e) => {
            tracing::warn!(
                "[noctalia-v5] Cannot snapshot plugin state for capture ({}); treating it as unknown (the manifest is still captured)",
                e
            );
            None
        }
    }
}

// ── Noctalia v5 path lookup ──────────────────────────────────────────

/// Palette colors the theme saved for itself, in display order. Prefers the
/// theme's own `palette.json` (custom palettes), falls back to `colors.json`
/// (a saved custom scheme). Empty when the palette comes from the wallpaper or
/// belongs to a palette HVE does not own — the panel then shows the name only,
/// and never invents colors.
fn palette_swatches(provider_dir: &Path) -> Vec<String> {
    const KEYS: [&str; 4] = ["mPrimary", "mSecondary", "mTertiary", "mSurface"];
    for file in ["palette.json", "colors.json"] {
        let Ok(raw) = fs::read_to_string(provider_dir.join(file)) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        // palette.json nests the scheme under `dark`/`light`; colors.json is flat.
        let scheme = value.get("dark").or_else(|| value.get("light")).unwrap_or(&value);
        let colors: Vec<String> = KEYS
            .iter()
            .filter_map(|k| scheme.get(*k).and_then(|c| c.as_str()).map(str::to_string))
            .collect();
        if !colors.is_empty() {
            return colors;
        }
    }
    Vec::new()
}

/// Displayable facts for a saved noctalia theme record: which palette the theme
/// points at (`custom <Name>`, `community <Name>`, `wallpaper <mode>`,
/// `builtin <Name>`), plus the colors themselves when the theme carries them.
fn theme_facts_from_records(provider_dir: &Path) -> Vec<ThemeFact> {
    let mut facts: Vec<ThemeFact> = Vec::new();

    if let Ok(text) = fs::read_to_string(provider_dir.join("source.txt")) {
        let mut parts = text.trim().splitn(2, char::is_whitespace);
        let kind = parts.next().unwrap_or("").trim();
        let name = parts.next().unwrap_or("").trim();
        if !kind.is_empty() {
            facts.push(ThemeFact {
                label: "Palette source".to_string(),
                value: kind.to_string(),
                swatches: Vec::new(),
            });
        }
        if !name.is_empty() {
            facts.push(ThemeFact {
                label: "Palette".to_string(),
                value: name.to_string(),
                swatches: Vec::new(),
            });
        }
    }

    let swatches = palette_swatches(provider_dir);
    if !swatches.is_empty() {
        facts.push(ThemeFact {
            label: "Palette colors".to_string(),
            value: String::new(),
            swatches,
        });
    }

    facts
}

/// Parse `custom <PaletteName>` from a `source.txt` body. Splits on ANY
/// whitespace (space, tab, …): HVE's own Save UI accepts names the old
/// space-only split misread, and a misread referrer is a palette deleted
/// while a surviving theme still needs it. `None` for non-custom records.
/// (Moved here from the theme manager with the `deletable-artefacts`
/// declaration: the record format is this backend's own knowledge.)
fn parse_custom_palette_name(text: &str) -> Option<String> {
    let mut parts = text.split_whitespace();
    if parts.next()? != "custom" {
        return None;
    }
    let name: String = parts.collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return None;
    }
    // Apply sanitizes `/` so the on-disk file can never contain one.
    Some(name.replace('/', "_"))
}

/// The file name Noctalia stores a COMMUNITY palette under: URL-ENCODED,
/// spaces as `%20`, with the `.json` extension every palette file carries.
/// `Catppuccin Mocha Sapphire` → `Catppuccin%20Mocha%20Sapphire.json`
/// (verified against the live `community-palettes` dir, 47 files, all
/// `<encoded name>.json`).
///
/// The pre-fix lookup joined the RAW name with no extension, so it built a
/// path that could not exist for any palette — the save warned and silently
/// kept the palette of an earlier save.
///
/// `%` itself is deliberately NOT encoded: a name that already carries
/// `%20` must not become `%2520`, and the raw fallback in
/// [`find_palette_path`] covers any name the encoded form misses.
fn community_palette_file_name(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    format!("{}.json", name.replace(' ', "%20"))
}

/// What a save must do with the theme's own `providers/<id>/palette.json`.
///
/// The invariant this serves: `palette.json` exists if and only if the
/// theme's declared source resolves to a palette. A save is a CAPTURE of
/// the live truth, so a palette that no longer resolves must not survive as
/// a leftover from an earlier save — the leftover that painted the
/// JokerTheme colours under a theme declaring a Catppuccin palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaletteCaptureAction {
    /// The declared palette resolved to a readable file: copy it in.
    Copy,
    /// Nothing resolves, or the source owns no palette at all: drop any
    /// `palette.json` the theme still carries.
    RemoveStale,
}

/// Decide the palette capture from the DECLARED source and the RESOLVED
/// file. Pure — no IPC, no filesystem, no logging — so the decision is
/// testable on its own.
///
/// `resolved` is the file [`find_palette_path`] returned *and* found on
/// disk; that function already answers `None` for `builtin`/`wallpaper`,
/// which own no palette file. An empty name declares nothing either way.
fn palette_capture_action(
    source: &str,
    name: &str,
    resolved: Option<&Path>,
) -> PaletteCaptureAction {
    if name.is_empty() || resolved.is_none() {
        return PaletteCaptureAction::RemoveStale;
    }
    match source {
        "custom" | "community" => PaletteCaptureAction::Copy,
        _ => PaletteCaptureAction::RemoveStale,
    }
}

/// Find the palette JSON file for a given source and name.
///
/// Sources:
/// - `custom`       → ~/.config/noctalia/palettes/<name>.json
/// - `builtin`      → No file — built-in palettes are internal to Noctalia
/// - `community`    → ~/.local/state/noctalia/community-palettes/<encoded name>
/// - `wallpaper`    → No file — extracted from wallpaper at runtime
fn find_palette_path(source: &str, name: &str) -> Option<PathBuf> {
    match source {
        "custom" => {
            noctalia_config_dir().map(|d| d.join("palettes").join(format!("{}.json", name)))
        }
        "community" => {
            // Encoded name FIRST (what Noctalia writes), raw as the
            // fallback so a palette stored under its unencoded name keeps
            // resolving. With neither on disk the encoded path comes back:
            // it is the canonical location, so a warning names the file
            // that is genuinely missing.
            let dir = noctalia_state_dir()?.join("community-palettes");
            let encoded = dir.join(community_palette_file_name(name));
            if encoded.is_file() {
                return Some(encoded);
            }
            let raw = dir.join(format!("{}.json", name));
            if raw.is_file() {
                return Some(raw);
            }
            Some(encoded)
        }
        _ => None,
    }
}

// ── Wallpaper-engine colour authority: detection + single-shot re-assert
//
// WHY this exists: the keeper runs the skwd-wall wallpaper engine as the
// color AUTHORITY (its config sets theme.policy == "wallpaper" and/or
// noctalia.themeMode == "follow"). HVE hands that engine the wallpaper
// during apply, so the engine regenerates its wallpaper-derived palette
// asynchronously AFTER HVE's synchronous steps and re-imposes
// `custom skwd-wall` — silently discarding the theme's custom palette for
// over a minute. HVE's palette copy is correct; it just loses the race
// without noticing. This section re-asserts the theme's palette once,
// bounded, off the UI thread, with honest logging.
//
// WHO owns what (capability routing): the engine's config file is the
// engine's business, so both the ownership READ and the yield/restore of
// the colour authority are reached through the `apply-background` router
// (`background::engine_owns_color_scheme`, `background::hold_color_authority`
// / `release_color_authority`, `background::color_authority_held`) — this
// provider never opens, flips or restores another backend's configuration.

/// Bounds for the single-shot re-assert: a short settle so the engine's
/// async regen can land first, at most 3 re-assert sets with a fixed delay
/// between checks, and a hard cap on the whole worker. Worst case the
/// worker lives settle + cap ≈ 10 s, then exits by itself — it can never
/// become a "keep watching" thread or a cross-engine war.
const COLOR_REASSERT_SETTLE: Duration = Duration::from_secs(2);
const COLOR_REASSERT_DELAY: Duration = Duration::from_secs(2);
const COLOR_REASSERT_MAX_SETS: u32 = 3;
const COLOR_REASSERT_CAP: Duration = Duration::from_secs(8);

/// Timing source: production constants, unless the `fast` test profile is
/// selected (same env-seam style as the rest of the codebase) so tests can
/// observe the full race end to end without waiting out the real delays.
fn color_reassert_timing() -> (Duration, u32, Duration, Duration) {
    if std::env::var("HVE_REASSERT_PROFILE").as_deref() == Ok("fast") {
        (
            Duration::from_millis(5),
            COLOR_REASSERT_MAX_SETS,
            Duration::from_millis(15),
            Duration::from_millis(1500),
        )
    } else {
        (
            COLOR_REASSERT_SETTLE,
            COLOR_REASSERT_MAX_SETS,
            COLOR_REASSERT_DELAY,
            COLOR_REASSERT_CAP,
        )
    }
}

/// Verify the custom palette is live and re-assert it when it is not.
///
/// Runs to completion on the calling thread: the apply path calls this via
/// [`spawn_custom_scheme_reassert`] so the UI thread never blocks. Each
/// check runs `color-scheme-get`; only a check that did NOT stick re-issues
/// `color-scheme-set custom <name>` + `templates-apply`. Returns true when
/// the scheme is verified live at the end. Never fails, never panics: every
/// IPC error is logged and swallowed, and a missing `noctalia` binary means
/// "do nothing" (the get fails, we log at debug and return).
fn reassert_custom_scheme_blocking(
    palette_name: &str,
    settle: Duration,
    max_sets: u32,
    delay: Duration,
    cap: Duration,
) -> bool {
    std::thread::sleep(settle);
    let expected = format!("custom {}", palette_name);
    let deadline = Instant::now() + cap;
    let mut sets = 0u32;
    loop {
        if Instant::now() >= deadline {
            tracing::warn!(
                "[noctalia-v5] custom scheme '{}' unverified at the cap: the background engine owns the color scheme — set skwd-wall-v2/config.json theme.policy away from \"wallpaper\" / noctalia.themeMode away from \"follow\", or the theme's saved palette will not stick",
                palette_name
            );
            return false;
        }
        match noctalia_msg(&["msg", "color-scheme-get"]) {
            Err(e) => {
                tracing::debug!(
                    "[noctalia-v5] color re-assert: cannot query scheme ({}); leaving it alone",
                    e
                );
                return false;
            }
            Ok(live) if live.trim() == expected => {
                if sets == 0 {
                    tracing::info!(
                        "[noctalia-v5] custom scheme '{}' stuck on first check — no re-assert needed",
                        palette_name
                    );
                } else {
                    tracing::info!(
                        "[noctalia-v5] custom scheme '{}' re-asserted and verified",
                        palette_name
                    );
                }
                return true;
            }
            Ok(live) => {
                if sets >= max_sets {
                    tracing::warn!(
                        "[noctalia-v5] custom scheme '{}' still overridden (live: '{}'): the background engine owns the color scheme — set skwd-wall-v2/config.json theme.policy away from \"wallpaper\" / noctalia.themeMode away from \"follow\", or the theme's saved palette will not stick",
                        palette_name,
                        live.trim()
                    );
                    return false;
                }
                if let Err(e) = noctalia_msg(&["msg", "color-scheme-set", "custom", palette_name])
                {
                    tracing::warn!(
                        "[noctalia-v5] color re-assert: cannot set scheme '{}': {}",
                        palette_name,
                        e
                    );
                } else if let Err(e) = noctalia_msg(&["msg", "templates-apply"]) {
                    tracing::warn!("[noctalia-v5] color re-assert: cannot apply templates: {}", e);
                }
                sets += 1;
                // Never sleep past the deadline: the cap is a hard wall time.
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .unwrap_or(Duration::ZERO);
                std::thread::sleep(delay.min(remaining));
            }
        }
    }
}

/// Live count of detached re-assert workers.
///
/// WHY it exists: a worker reads PROCESS-GLOBAL state — `PATH` to resolve the
/// `noctalia` stub, `SKWD_WALL_V2_CONFIG`/`HOME` to reach the engine config
/// and the colour-authority descriptor. The test harness restores those
/// globals when a test ends, so a worker that outlives its test would run
/// against the NEXT test's sandbox and leak a `color-scheme-set` into its
/// stub log or write the engine config behind its back. The harness waits on
/// this counter (see `ColorStub::drop`) before releasing the sandbox.
///
/// Production never reads it; it only guards the test sandbox.
static REASSERT_WORKERS_ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// Decrements [`REASSERT_WORKERS_ACTIVE`] when the worker ends, panic or not.
struct ReassertWorkerGuard;

impl Drop for ReassertWorkerGuard {
    fn drop(&mut self) {
        REASSERT_WORKERS_ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Whether every detached re-assert worker has finished. Test-only: the
/// sandbox harness polls it before restoring its process-global environment.
#[cfg(test)]
pub(crate) fn reassert_workers_idle() -> bool {
    REASSERT_WORKERS_ACTIVE.load(Ordering::SeqCst) == 0
}

/// Fire-and-forget the single-shot re-assert after an apply restored a
/// custom palette while the engine owns the scheme.
///
/// Detached by design: the engine's regen lands after apply returns, so
/// joining here would freeze the UI thread for up to ~10 s. The worker
/// never touches Slint state — it only runs `noctalia msg` and logs — so
/// detaching is safe, and it always exits by itself at the attempt cap.
///
/// `previous_value`: when the apply yielded the engine's colour authority
/// (W2), the worker restores it once the palette is verified (or at the
/// bounded cap); `None` keeps today's behaviour byte-for-byte.
fn spawn_custom_scheme_reassert(palette_name: String, previous_value: Option<String>) {
    REASSERT_WORKERS_ACTIVE.fetch_add(1, Ordering::SeqCst);
    // The guard is built in the PARENT and moved into the worker, and the
    // spawn goes through `Builder::spawn` (a `Result`, never a panic). Both
    // together close the leak the plain `thread::spawn` left open: a spawn
    // that fails drops the closure in the parent, so the moved guard
    // decrements and the count can never stay at +1 for the rest of the
    // process (which would make every later sandbox wait its full deadline).
    let active = ReassertWorkerGuard;
    let spawned = std::thread::Builder::new().spawn(move || {
        let _active = active;
        let (settle, max_sets, delay, cap) = color_reassert_timing();
        reassert_custom_scheme_blocking(&palette_name, settle, max_sets, delay, cap);
        // The palette is now verified live (or the bounded cap was hit, or
        // the IPC hardened): put the engine's colour authority back EXACTLY
        // as it was. Runs on every worker exit path, so the restore is
        // never lost — the guard disarmed itself in favour of this thread.
        if let Some(previous) = previous_value {
            match background::release_color_authority(&previous) {
                Ok(wrote) => tracing::info!(
                    "[noctalia-v5] engine colour authority restored (theme.policy -> \"{}\", wrote={})",
                    previous,
                    wrote
                ),
                Err(e) => tracing::warn!(
                    "[noctalia-v5] could not restore engine colour authority: {}",
                    e
                ),
            }
        }
    });
    if let Err(e) = spawned {
        // The closure (and its guard) was dropped above, so the count is
        // already back. Say it out loud: a silent failure here would look
        // exactly like "no worker was ever needed".
        tracing::warn!("[noctalia-v5] could not spawn the colour re-assert worker: {}", e);
    }
}

/// Whether the live Noctalia colour state still matches the theme's saved
/// palette snapshot (unit 1c2 of `odd/tasks/hve-capability-routing.md`).
///
/// Pure: no I/O, no `Command` — `reassert_colours` reads the inputs and this
/// function decides, so the decision is pinned by unit tests.
///
/// `None` live scheme (noctalia not answering / no scheme readable) answers
/// `true`: the re-assert is idempotent (copy + set + templates converge on
/// the snapshot), so an unreadable scheme cannot start a fight; a dead
/// daemon surfaces as `Err` from the CLI step instead. An empty snapshot or
/// an empty name owns nothing and answers `false`.
fn palette_needs_reassert(
    live_scheme: Option<&str>,
    live_palette: Option<&[u8]>,
    snapshot: &[u8],
    palette_name: &str,
) -> bool {
    if snapshot.is_empty() || palette_name.is_empty() {
        return false;
    }
    if live_scheme.is_none() {
        return true;
    }
    let expected = format!("custom {}", palette_name);
    if live_scheme.map(|s| s.trim()) != Some(expected.as_str()) {
        return true;
    }
    !matches!(live_palette, Some(live) if live == snapshot)
}

// ── The Noctalia shell-capability adapter ────────────────────────────
//
// The core drives Noctalia through this adapter, never by name. It owns
// every Noctalia-specific fact the core needs: the CLI calls, the
// `providers/noctalia-v5/` record layout, and the DND spelling.

pub struct NoctaliaShell;

impl crate::providers::shell_capabilities::ShellCapabilities for NoctaliaShell {
    fn live_wallpaper(&self) -> Option<String> {
        noctalia_msg(&["msg", "wallpaper-get"])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn saved_theme_wallpaper(
        &self,
        themes_root: &std::path::Path,
        theme_name: &str,
    ) -> Option<String> {
        std::fs::read_to_string(
            themes_root
                .join(theme_name)
                .join("providers")
                .join("noctalia-v5")
                .join("wallpaper.txt"),
        )
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    }

    fn arms_palette_reassert(&self, themes_root: &std::path::Path, theme_name: &str) -> bool {
        let provider_dir = themes_root
            .join(theme_name)
            .join("providers")
            .join("noctalia-v5");
        let raw = match std::fs::read_to_string(provider_dir.join("source.txt")) {
            Ok(raw) => raw,
            Err(_) => return false,
        };
        let parts: Vec<&str> = raw.trim().splitn(2, ' ').collect();
        let name = parts.get(1).copied().unwrap_or("");
        if name.is_empty() {
            return false;
        }
        provider_dir.join("palette.json").is_file()
    }

    fn dnd_status(&self) -> Option<String> {
        let out = std::process::Command::new("noctalia")
            .args(["msg", "notification-dnd-status"])
            .output()
            .ok()?;
        let s = String::from_utf8(out.stdout).ok()?;
        let s = s.trim().to_string();
        if s == "on" || s == "off" {
            Some(s)
        } else {
            None
        }
    }

    fn dnd_set(&self, state: &str) -> bool {
        let Some(arg) = dnd_bool_arg(state) else {
            return false;
        };
        std::process::Command::new("noctalia")
            .args(["msg", "notification-dnd-set", arg])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("ok"))
            .unwrap_or(false)
    }
}

/// "on"→"true", "off"→"false" — Noctalia's DND boolean spelling. Anything
/// else is not a status the shell reports, so it is refused without spawning.
fn dnd_bool_arg(status: &str) -> Option<&'static str> {
    match status {
        "on" => Some("true"),
        "off" => Some("false"),
        _ => None,
    }
}

/// The packaging bound a save must use. Production: the fixed constant, which
/// never reads the environment. Under `cfg(test)` the `cfg(test)`-only seam is
/// used so the size decision can be exercised with tiny files.
#[cfg(test)]
fn save_packaging_bound() -> u64 {
    crate::theme_media::test_packaged_video_bytes()
}

/// See the `cfg(test)` overload above.
#[cfg(not(test))]
fn save_packaging_bound() -> u64 {
    crate::theme_media::max_packaged_video_bytes()
}

// ── NoctaliaV5Provider ───────────────────────────────────────────────
//
// Self-contained: the whole save/apply happens over IPC (`noctalia msg`).
// It needs no ShellProvider, no file paths, no template processor.

pub struct NoctaliaV5Provider;

impl NoctaliaV5Provider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoctaliaV5Provider {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeProvider for NoctaliaV5Provider {
    fn id(&self) -> &str {
        "noctalia-v5"
    }

    fn display_name_key(&self) -> &str {
        "themes.provider.noctalia_v5"
    }

    fn icon(&self) -> &str {
        "◈"
    }

    fn shell(&self) -> &str {
        "noctalia-v5"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::COLORS | ProviderCapabilities::WALLPAPERS
    }

    /// The `read-preview-source` declarations for the three preview
    /// records this provider saves in its own directory: the live
    /// background assignment manifest (format knowledge in `bg_info`, the
    /// module that owns that record), the painter-identified video record
    /// (format knowledge in `wallpaper_authority`), and the static
    /// wallpaper text record. Each declaration carries the record's own
    /// candidate order; the gallery core decides existence, precedence and
    /// fall-through.
    fn preview_sources(&self, provider_dir: &Path) -> Vec<PreviewSource> {
        let mut out = Vec::new();

        let paths = crate::providers::bg_info::preview_candidates(provider_dir);
        if !paths.is_empty() {
            out.push(PreviewSource {
                role: PreviewRole::Assignment,
                kind: PreviewSourceKind::Video,
                paths,
            });
        }

        if let Ok(text) = fs::read_to_string(provider_dir.join(wallpaper_authority::PAINTER_VIDEO_FILE))
        {
            if let Some(path) = wallpaper_authority::parse_painter_video_record(&text) {
                out.push(PreviewSource {
                    role: PreviewRole::PainterVideo,
                    kind: PreviewSourceKind::Video,
                    paths: vec![path],
                });
            }
        }

        if let Ok(text) = fs::read_to_string(provider_dir.join("wallpaper.txt")) {
            if let Some(line) = text.lines().map(str::trim).find(|l| !l.is_empty()) {
                out.push(PreviewSource {
                    role: PreviewRole::WallpaperText,
                    kind: PreviewSourceKind::Image,
                    paths: vec![PathBuf::from(line)],
                });
            }
        }

        out
    }

    /// The remote video url this provider's own mpvpaper manifest knows for
    /// `source` (theme-packages T3). The manifest format is this backend's
    /// own record knowledge (`bg_info`), so the parse stays here and the core
    /// only records what the provider states.
    fn video_url(&self, provider_dir: &Path, source: &Path) -> Option<ThemeVideoUrl> {
        crate::providers::bg_info::video_url_for(provider_dir, source).map(|known| ThemeVideoUrl {
            url: known.url,
            filename: known.filename,
            sha256: known.sha256,
        })
    }

    /// The theme's own palette record, for the gallery info panel: the saved
    /// palette colors when the theme carries them, and always which palette it
    /// points at (`custom <Name>`, `community <Name>`, `wallpaper <mode>`,
    /// `builtin <Name>`). A wallpaper-derived palette has no color file, so the
    /// panel shows the palette NAME and invents nothing.
    fn theme_facts(&self, provider_dir: &Path) -> Vec<ThemeFact> {
        theme_facts_from_records(provider_dir)
    }

    fn save(&self, theme_dir: &Path) -> Result<(), String> {
        let provider_dir = theme_dir.join("providers").join(self.id());
        fs::create_dir_all(&provider_dir)
            .map_err(|e| format!("Cannot create provider dir: {}", e))?;

        // 1. Get active color scheme via IPC
        let scheme = noctalia_msg(&["msg", "color-scheme-get"])
            .map_err(|e| format!("Cannot get color scheme: {}", e))?;
        let scheme = scheme.trim().to_string();
        if scheme.is_empty() {
            return Err("Empty color-scheme-get response".into());
        }

        // Save scheme source info (e.g. "custom JokerTheme")
        fs::write(provider_dir.join("source.txt"), &scheme)
            .map_err(|e| format!("Cannot write source.txt: {}", e))?;

        // Parse source and name
        let parts: Vec<&str> = scheme.splitn(2, ' ').collect();
        let source = parts.first().copied().unwrap_or("custom");
        let name = parts.get(1).copied().unwrap_or("");

        // 2. Save palette JSON — or make sure the theme carries NO palette it
        //    does not own. A save is a CAPTURE of the live truth: a theme
        //    whose declared palette cannot be resolved must not keep the
        //    `palette.json` an EARLIER save left behind, because apply would
        //    then paint those foreign colours under this theme's palette name
        //    (the defect: a theme declaring a Catppuccin community palette
        //    painted the JokerTheme palette). The invariant held here:
        //    `palette.json` exists if and only if the declared source
        //    resolves to a palette.
        let palette_dest = provider_dir.join("palette.json");
        let resolved_palette: Option<PathBuf> =
            find_palette_path(source, name).filter(|path| path.is_file());
        match palette_capture_action(source, name, resolved_palette.as_deref()) {
            PaletteCaptureAction::Copy => {
                let from = resolved_palette.as_deref().ok_or_else(|| {
                    "Cannot copy palette: the capture decision has no resolved file".to_string()
                })?;
                fs::copy(from, &palette_dest)
                    .map_err(|e| format!("Cannot copy palette: {}", e))?;
                tracing::info!("[noctalia-v5] Saved palette from {:?}", from);
            }
            PaletteCaptureAction::RemoveStale => {
                tracing::warn!(
                    "[noctalia-v5] No palette resolves for theme {} (source={}, name={}); \
                     the theme must not keep a palette it does not own",
                    theme_dir.display(),
                    source,
                    name
                );
                remove_stale_artifact(&palette_dest);
            }
        }

        // 3. Ask the compositor which background is actually on top (the
        //    daemon is a secondary signal only), and capture ONLY that one.
        //    Static -> wallpaper.txt only (drop any stale video record);
        //    video with a resolvable painter path -> the painter record
        //    (video.txt) only (drop the stale mpvpaper manifest, which still
        //    names the previous video). Anything unverifiable — no signal,
        //    or video with no resolvable path — keeps the legacy
        //    capture-everything behaviour so a missing/unreachable query
        //    never loses data silently, and says so.
        //
        //    Two-tier decision from one snapshot: the cheap kind-only
        //    decision sets the plan; the richer background decision (kind +
        //    path + painter) runs only when a video is on top and its path
        //    is needed. Both read the same captured inputs, so the kind
        //    cannot flip between them; a video decision without a
        //    resolvable path still vetoes honestly instead of recording a
        //    video that may no longer be showing.
        let authority = wallpaper_authority::query_authority_snapshot();
        let plan = match wallpaper_authority::snapshot_wallpaper_kind(&authority) {
            Ok(kind) => wallpaper_authority::plan_from_kind(Some(kind)),
            Err(reason) => {
                tracing::warn!(
                    "[noctalia-v5] No wallpaper signal ({}); capturing available backgrounds",
                    reason
                );
                SavePlan::Unknown
            }
        };
        // A video kind is exact only when the painter's path resolves:
        // without it there is nothing truthful to record, so veto honestly
        // (legacy capture) instead of inventing a path.
        let video_bg: Option<wallpaper_authority::ActiveBackground> =
            if matches!(plan, SavePlan::VideoOnly) {
                match wallpaper_authority::snapshot_background(&authority) {
                    Ok(bg)
                        if bg.kind == WallpaperKind::Video && bg.video_path.is_some() =>
                    {
                        Some(bg)
                    }
                    Ok(other) => {
                        tracing::warn!(
                            "[noctalia-v5] Video on top but painter identity unavailable ({:?}); capturing available backgrounds",
                            other.kind
                        );
                        None
                    }
                    Err(reason) => {
                        tracing::warn!(
                            "[noctalia-v5] Video on top but painter identity unavailable ({}); capturing available backgrounds",
                            reason
                        );
                        None
                    }
                }
            } else {
                None
            };
        let exact_video = video_bg.is_some();
        let vetoed_video = matches!(plan, SavePlan::VideoOnly) && !exact_video;
        let capture_static =
            matches!(plan, SavePlan::StaticOnly | SavePlan::Unknown) || vetoed_video;
        let capture_video_manifest = matches!(plan, SavePlan::Unknown) || vetoed_video;

        // The background the theme actually carries (theme-packages T1):
        // whichever source the authority capture resolved to — the static
        // wallpaper or the painter video — is packaged under `{theme}/media/`
        // after the absolute records are written, and named by a
        // theme-relative `poster.txt` record. The absolute records above stay
        // byte-for-byte, so an existing theme on disk keeps resolving exactly
        // as before.
        let mut poster_source: Option<PathBuf> = None;
        // The theme's own VIDEO source (theme-packages T3): the exact painter
        // video when the authority resolved one. Packaged under `{theme}/media/`
        // when it fits the size bound, or recorded by url when it does not.
        let mut video_source: Option<PathBuf> = None;

        // 4. Save default wallpaper (static only, unless authority unknown).
        //
        // WHY the daemon override: the suite that paints the layer keeps
        // its own per-output truth (`skwd-helm outputs --json`), while
        // `wallpaper-get` is a third party value that can lag it (seen
        // live: Noctalia reported car3.jpg while the suite painted
        // joker1.png). When the compositor proves the skwd suite paints a
        // static scene, the recorded path is the daemon's `current` for
        // the lexicographically-first painted output (see
        // `resolve_static_path` for the rule); any other painter, an
        // unreachable compositor, or an unusable daemon keeps today's
        // `wallpaper-get` value byte-for-byte, with a warning naming the
        // output when the suite paints but the daemon cannot complete it.
        if capture_static {
            let legacy: Option<String> = match noctalia_msg(&["msg", "wallpaper-get"]) {
                Ok(wp) => {
                    let wp = wp.trim().to_string();
                    if wp.is_empty() {
                        None
                    } else {
                        Some(wp)
                    }
                }
                Err(e) => {
                    tracing::warn!("[noctalia-v5] Cannot get wallpaper: {}", e);
                    None
                }
            };
            let recorded: Option<String> = match wallpaper_authority::snapshot_static_path(
                &authority,
            ) {
                Ok(Some(current)) => {
                    let current = current.trim().to_string();
                    if current.is_empty() {
                        legacy
                    } else {
                        if legacy.as_deref() != Some(current.as_str()) {
                            tracing::info!(
                                "[noctalia-v5] Recording skwd daemon current as wallpaper (the suite paints it): {}",
                                current
                            );
                        }
                        Some(current)
                    }
                }
                Ok(None) => legacy,
                Err(reason) => {
                    tracing::warn!(
                        "[noctalia-v5] {} — recording wallpaper-get value instead",
                        reason
                    );
                    legacy
                }
            };
            if let Some(wp) = recorded {
                fs::write(provider_dir.join("wallpaper.txt"), &wp)
                    .map_err(|e| format!("Cannot write wallpaper.txt: {}", e))?;
                poster_source = Some(PathBuf::from(&wp));
            }
        } else {
            // Authority says video: a stale static record would resurrect the
            // wrong background, so remove it (re-save does not wipe the dir).
            remove_stale_artifact(&provider_dir.join("wallpaper.txt"));
        }

        // 5. Save full Noctalia v5 settings (bar, OSD, widgets, layout, everything)
        //    v5 stores its settings in ~/.local/state/noctalia/settings.toml, NOT
        //    in ~/.config/noctalia/settings.json (which is a v4 artifact).
        //
        //    The copy is then SANITIZED: the live file names this machine's
        //    paths (sddm hook scripts, wallpaper folders, screenshot dirs), and
        //    a theme that carries them overwrites a foreign machine's settings
        //    with ours on apply. The look travels; the machine's paths do not.
        let v5_settings = noctalia_state_dir().map(|d| d.join("settings.toml"));
        if let Some(settings_path) = v5_settings {
            if settings_path.exists() {
                let dest = provider_dir.join("settings.toml");
                fs::copy(&settings_path, &dest)
                    .map_err(|e| format!("Cannot copy settings.toml: {}", e))?;
                sanitize_settings_file(&dest, theme_dir);
                tracing::info!("[noctalia-v5] Saved settings.toml");
            }
        }

        // 6. Painter-identified video record (video.txt): kind + path + who
        //    painted it, so apply can restore it exactly through the manager
        //    in charge. Written only for an exact video; every other arm
        //    drops a stale record (re-save does not wipe the dir, and a
        //    leftover would hijack apply with a video no longer on top).
        //    A video KIND with no resolvable path records nothing — the
        //    legacy capture below stays honest instead of inventing one.
        match video_bg.as_ref() {
            Some(bg) => match bg.video_path.as_ref() {
                Some(path) => {
                    let record = wallpaper_authority::format_painter_video_record(
                        path,
                        bg.painter_proc.as_deref(),
                        &bg.top_namespace,
                        bg.top_pid,
                    );
                    match fs::write(
                        provider_dir.join(wallpaper_authority::PAINTER_VIDEO_FILE),
                        &record,
                    ) {
                        Ok(()) => tracing::info!(
                            "[noctalia-v5] Saved painter video record (video.txt): {}",
                            path.display()
                        ),
                        Err(e) => {
                            tracing::warn!("[noctalia-v5] Cannot write video.txt: {}", e)
                        }
                    }
                    poster_source = Some(path.clone());
                    video_source = Some(path.clone());
                }
                // Unreachable by construction (video_bg only holds resolved
                // paths), but honesty first: never write a record without one.
                None => tracing::warn!(
                    "[noctalia-v5] Exact video lost its identity; no video.txt written"
                ),
            },
            None => {
                remove_stale_artifact(&provider_dir.join(wallpaper_authority::PAINTER_VIDEO_FILE));
            }
        }

        // 6b. Package the theme's own background (theme-packages T1). The
        //     source is whichever the authority capture resolved to: a video
        //     frame as native-resolution JPEG, or the static wallpaper copied
        //     byte-for-byte. Best-effort by contract — a missing source, a
        //     missing ffmpeg or a failed extraction only warns; the save
        //     still succeeds and existing absolute records are unchanged.
        //
        //     INVARIANT (correction): `poster.txt` and `media/poster.*`
        //     describe the theme's CURRENT background, or they do not exist.
        //     T2 made apply resolve the theme poster FIRST, so whenever the
        //     background is NOT successfully (re)packaged — a failed
        //     extraction, no ffmpeg, a failed record write, or no background
        //     source at all — the stale poster is removed and apply falls back
        //     to the normal chain instead of painting the old image.
        let packaged = match poster_source.as_deref() {
            Some(source) => match crate::theme_media::write_theme_poster(theme_dir, source) {
                Some(relative) => {
                    match crate::theme_media::write_poster_record(&provider_dir, &relative) {
                        Ok(()) => true,
                        Err(e) => {
                            tracing::warn!(
                                "[noctalia-v5] Cannot record the packaged poster ({}); \
                                 dropping the stale theme poster",
                                e
                            );
                            false
                        }
                    }
                }
                None => {
                    tracing::warn!(
                        "[noctalia-v5] Background not packaged ({}) — save continues",
                        source.display()
                    );
                    false
                }
            },
            None => false,
        };
        if !packaged {
            crate::theme_media::remove_theme_poster(theme_dir, &provider_dir);
        }

        // 6c. Package the theme's own video (theme-packages T3), size-aware.
        //     A video at or under the packaging bound is copied into the theme
        //     as `media/background.mp4` and named by a theme-relative record;
        //     a larger one is never copied — the url the theme's manifest
        //     already knows is recorded instead, so the video can still be
        //     fetched by the mpvpaper backend. No url means the theme travels
        //     with its poster alone. Best-effort by contract: a missing file,
        //     an over-bound video or a failed copy only warns; the save still
        //     succeeds.
        //
        //     INVARIANT (mirrors the poster): `video-media.txt` and
        //     `media/background.mp4` describe the theme's CURRENT video, or
        //     they do not exist. A save whose background is not a video, or a
        //     video that could not be packaged and has no known url, removes
        //     both. Read the manifest BEFORE step 7 drops it for an exact
        //     video, so an over-bound video keeps the url it knew.
        let video_packaged = match video_source.as_deref() {
            Some(source) => {
                match crate::theme_media::package_theme_video(
                    theme_dir,
                    source,
                    save_packaging_bound(),
                ) {
                    Some(relative) => {
                        match crate::theme_media::write_video_record(&provider_dir, &relative) {
                            Ok(()) => true,
                            Err(e) => {
                                tracing::warn!(
                                    "[noctalia-v5] Cannot record the packaged video ({}); \
                                     dropping the stale theme video",
                                    e
                                );
                                false
                            }
                        }
                    }
                    None => match crate::providers::bg_info::video_url_for(&provider_dir, source) {
                        Some(known) => {
                            match crate::theme_media::write_video_url_record(
                                &provider_dir,
                                &known.url,
                                known.filename.as_deref(),
                                known.sha256.as_deref(),
                            ) {
                                Ok(()) => true,
                                Err(e) => {
                                    tracing::warn!(
                                        "[noctalia-v5] Cannot record the video url ({}); \
                                         the theme keeps only its poster",
                                        e
                                    );
                                    false
                                }
                            }
                        }
                        None => {
                            tracing::warn!(
                                "[noctalia-v5] Video not packaged ({}) and no url known; \
                                 the theme travels with its poster",
                                source.display()
                            );
                            false
                        }
                    },
                }
            }
            None => false,
        };
        if !video_packaged {
            crate::theme_media::remove_theme_video(theme_dir, &provider_dir);
        }

        // 6d. Drop the absolute background records. `wallpaper.txt` and
        //     `video.txt` name files on THIS machine, so a theme carrying them
        //     hands a foreign machine the keeper's paths — or, for the sddm
        //     hook case, someone else's script. The packaged `poster.txt` /
        //     `video-media.txt` written above is what a target machine
        //     resolves. A record with no packaged equivalent still goes (the
        //     foreign absolute path is the defect), and says so in the log.
        drop_absolute_background_records(&provider_dir, packaged, video_packaged);

        // 7. Save animated wallpaper manifest (mpvpaper plugin), reference only.
        //    Only for the honest legacy capture (unreachable authority, or a
        //    vetoed video with no painter path): the manifest comes from
        //    mpvpaper's state file, which still names the previous video, so
        //    an exact painter record must never share a theme with it — apply
        //    would otherwise have two videos claiming the screen.
        //
        //    The manifest is also gated on the plugin being able to paint: a
        //    provably DISABLED `noctalia/mpvpaper` owns nothing, so its live
        //    assignments file is dead and copying it would let a stale video
        //    hijack the theme's preview and re-apply (the gallery ranks a
        //    live assignment above the theme's own static record). The
        //    capture-specific snapshot never folds an unreadable state into
        //    "disabled": it falls back to the IPC and only `Some(false)` —
        //    a provable disabled — suppresses the copy, so a format drift
        //    cannot silently stop capturing an enabled plugin's record.
        //    Read the tri-state once and reuse it.
        let plugin_enabled = if capture_video_manifest {
            mpvpaper_can_paint_for_capture()
        } else {
            None
        };
        if should_capture_video_manifest(capture_video_manifest, plugin_enabled) {
            match crate::providers::bg_info::save_manifest(&provider_dir) {
                Ok(()) => {}
                Err(e) => tracing::warn!("[noctalia-v5] Could not save mpvpaper manifest: {}", e),
            }
        } else {
            if capture_video_manifest {
                tracing::info!(
                    "[noctalia-v5] Skipping the mpvpaper manifest capture: noctalia/mpvpaper is disabled, so its live assignments file is stale"
                );
            }
            // Authority decided exactly (static, or video with a painter
            // record) or the plugin is disabled: drop any stale manifest
            // (re-save does not wipe the dir, and a leftover would hijack
            // apply).
            remove_stale_artifact(&provider_dir.join(crate::providers::bg_info::MANIFEST_FILE));
        }

        Ok(())
    }

    fn apply(&self, theme_dir: &Path) -> Result<(), String> {
        let provider_dir = theme_dir.join("providers").join(self.id());
        if !provider_dir.exists() {
            return Err("Noctalia v5 provider data not found in theme".into());
        }

        let palette_src = provider_dir.join("palette.json");
        let source_src = provider_dir.join("source.txt");

        // 0. Restore full Noctalia v5 settings FIRST (bar, OSD, widgets, layout, etc.)
        //    v5 stores settings in ~/.local/state/noctalia/settings.toml, NOT
        //    in settings.json (v4 artifact). This must happen before wallpaper/scheme
        //    so config-reload loads the complete ecosystem state first.
        //
        //    D4: the saved settings.toml carries its own `[plugins] enabled`
        //    list (every saved theme has `noctalia/mpvpaper` enabled while
        //    the keeper runs disabled), so restoring it verbatim plus
        //    `config-reload` would re-enable a plugin the user turned off —
        //    and its boot `applyAll()` would relaunch the stale assignments
        //    video. Snapshot the pre-apply state first and re-assert it
        //    after the reload; the rest of the settings restore is
        //    untouched.
        let plugin_was_enabled = live_mpvpaper_plugin_enabled();
        let mut settings_restored = false;
        // U7 + U16 of `odd/tasks/noctalia-layout-survives-the-apply.md`:
        // the layout-verify duty is armed here, the moment the apply
        // writes the live settings.toml, and the worker is SPAWNED by the
        // guard's `Drop` — on the normal exit AND on every `?` return
        // below, so an Err after step 0 can never strand the live file
        // unchecked. The refuted delivery verified right after step 0
        // while the apply still had later writes and reloads to go (the
        // wallpaper-rotation neutralize writes the live file and reloads,
        // then `wallpaper-set`, `color-scheme-set`, `templates-apply`),
        // so a wipe landing on any of them was never caught. U9: the
        // verify runs OFF the UI thread — see `spawn_layout_verify_and_repair`.
        let spawn_duty = LayoutVerifySpawnDuty(std::cell::Cell::new(None));
        let saved_settings = provider_dir.join("settings.toml");
        if saved_settings.exists() {
            if let Some(state_dir) = noctalia_state_dir() {
                let dst = state_dir.join("settings.toml");
                // U16: the generation is bumped HERE, immediately before
                // the FIRST write to the live settings.toml — supersession
                // coincides with taking the duty (see [`APPLY_GENERATION`]).
                // The stale first-statement bump made a failed apply cancel
                // its predecessor's worker without doing anything of its
                // own.
                let generation = APPLY_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
                fs::copy(&saved_settings, &dst)
                    .map_err(|e| format!("Cannot restore settings.toml: {}", e))?;
                settings_restored = true;
                tracing::info!("[noctalia-v5] Restored settings.toml");

                // U16: the duty is taken as soon as the live file was
                // touched — from here on the guard spawns the verify on
                // EVERY exit path of the apply, Ok or Err. The reloaded
                // config may still wipe the layout even when a later step
                // fails, so a failure can never strand the live file
                // unchecked.
                spawn_duty
                    .0
                    .set(Some((saved_settings.clone(), dst.clone(), generation)));

                noctalia_msg(&["msg", "config-reload"])
                    .map_err(|e| format!("Cannot reload config: {}", e))?;
                tracing::info!("[noctalia-v5] Config reloaded — bar, OSD, widgets restored");

                match plugin_reassert_action(plugin_was_enabled) {
                    PluginReassert::Disable => {
                        if let Err(e) =
                            noctalia_msg(&["msg", "plugins", "disable", "noctalia/mpvpaper"])
                        {
                            tracing::warn!(
                                "[noctalia-v5] Could not keep noctalia/mpvpaper disabled: {}",
                                e
                            );
                        } else {
                            tracing::info!(
                                "[noctalia-v5] Kept noctalia/mpvpaper disabled (pre-apply state)"
                            );
                        }
                    }
                    PluginReassert::Enable => {
                        if let Err(e) =
                            noctalia_msg(&["msg", "plugins", "enable", "noctalia/mpvpaper"])
                        {
                            tracing::warn!(
                                "[noctalia-v5] Could not keep noctalia/mpvpaper enabled: {}",
                                e
                            );
                        }
                    }
                    PluginReassert::Leave => tracing::warn!(
                        "[noctalia-v5] Unknown pre-apply plugin state; leaving reloaded plugins as-is"
                    ),
                }

                // U2 of `odd/tasks/noctalia-layout-survives-the-apply.md`
                // (corrected by U6/U7 after the verification refuted the
                // first delivery, and by U9 after the keeper felt the
                // settle's UI freeze): Noctalia re-persists its in-memory
                // config ~1 s after the reload above and can WIPE the
                // layout sections the restore just put back (the
                // ratchet). The verify + bounded surgical repair are
                // SPAWNED by the duty guard's `Drop` (U16) — after the
                // apply body, whether it returned Ok or Err
                // (`spawn_layout_verify_and_repair` runs off the UI
                // thread) — so no later step can strand the state, and
                // the wait never blocks the UI thread. U12: the duty
                // carries this apply's generation, so the worker can
                // yield to a newer apply instead of overwriting its file.
            }
        }

        // 1. Painter-identified video (video.txt): the exact record of what
        //    was on top at save time (kind + path + who painted it).
        //    Restored through the manager in charge (`skwd-helm apply` via
        //    the delegation helper) — the mpvpaper plugin is never touched,
        //    so a plugin the user disabled stays disabled (G3). When the
        //    record is missing or unresolvable, fall through to the legacy
        //    manifest path below; a theme without any video record still
        //    sends clear-all first so a running video cannot hijack the
        //    screen. Best-effort throughout: a video problem must never fail
        //    the whole theme apply.
        let mut animated_applied = false;
        // The exact video path restored through the painter record (video.txt),
        // kept for the Phase 1 poster hand-off below. `None` when no painter
        // video was restored (legacy manifest, static, or delegation failure).
        let mut applied_video: Option<PathBuf> = None;
        let painter_record = provider_dir.join(wallpaper_authority::PAINTER_VIDEO_FILE);
        // T3 theme-first video: the packaged `media/background.mp4` wins over
        // the absolute `video.txt` path, which in turn wins over a recorded
        // url. `None` for every old theme (no record, no media), so their
        // chain stays byte-for-byte as before.
        let recorded_absolute: Option<PathBuf> = fs::read_to_string(&painter_record)
            .ok()
            .and_then(|text| wallpaper_authority::parse_painter_video_record(&text));
        let theme_video = crate::theme_media::resolve_theme_video(
            theme_dir,
            &provider_dir,
            recorded_absolute.as_deref(),
        );
        // ── Colour-authority yield (W2 + W4) ────────────────────────────
        // The keeper's wallpaper engine owns the colour scheme
        // (`theme.policy == "wallpaper"`): if a wallpaper is handed over
        // with the policy still on, the engine regenerates its wallpaper
        // palette asynchronously AFTER these synchronous steps and
        // re-imposes it — the ~4 s flash the keeper measured. Flip the
        // policy to `off` BEFORE the first hand-off, give the engine's
        // config reload a bounded moment to observe the flip, and hold the
        // authority off until the theme palette is verified. The previous
        // value comes back EXACTLY as it was, and a failed write never
        // aborts the apply. The whole mechanism lives behind the
        // `apply-background` router (`background::hold_color_authority`),
        // because this provider never configures another backend.
        //
        // W2 wired the yield into the STATIC branch only; W4 hoists it
        // before the WHOLE wallpaper-restoration phase so the animated
        // hand-off (video.txt -> skwd-helm apply, below) is covered too.
        // One yield per apply — whichever branch hands the wallpaper over
        // first (the video delegation when the animated branch runs, the
        // static `wallpaper-set` otherwise), the flip already happened and
        // the observe wait already elapsed. The hold returns its elapsed
        // time so the keeper's live test can tune the router's bound.
        //
        // Yield only when a wallpaper is actually going to change hands: a
        // painter video record exists (the delegation below) or a static
        // wallpaper.txt names a non-empty path (the restore further down).
        // The static read is LENIENT here — the authoritative read with
        // its honest error stays in the static branch — so an unreadable
        // wallpaper.txt cannot change the error behaviour of today. A
        // theme with neither asset hands nothing over: nothing to yield,
        // the engine config stays byte-identical (today's behaviour).
        //
        // Ownership decision BEFORE the flip: after the flip the config
        // reads `off`, and `background::engine_owns_color_scheme()` would
        // answer FALSE — denying the very re-assert that must run (the W2
        // trap). This snapshot powers the spawn decision below, and the
        // yielded value rides along to the worker explicitly so the
        // restore is never lost.
        let wp_path = provider_dir.join("wallpaper.txt");
        // Same lenient read as before (an unreadable wallpaper.txt must not
        // change the yield behaviour): the non-empty saved path, kept as a
        // fact for the `apply-background` request built below.
        let static_wallpaper = fs::read_to_string(&wp_path)
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .map(PathBuf::from);
        // T2 theme-first: the theme's own packaged poster, resolved against
        // the theme dir NOW. `None` for every old theme (no record, no
        // media), so their yield decision and background chain stay
        // byte-for-byte as before.
        let theme_poster = crate::theme_media::resolve_theme_poster(theme_dir, &provider_dir);
        let static_planned = static_wallpaper.is_some() || theme_poster.is_some();
        // A theme-first video (packaged, absolute or url) also changes hands,
        // so it takes part in the yield decision exactly like the records it
        // replaces.
        let video_planned = theme_video.is_some();
        let owner_snapshot: Option<bool>;
        let mut authority_guard: Option<background::ColourAuthorityHold> = None;
        if painter_record.exists() || static_planned || video_planned {
            owner_snapshot = Some(background::engine_owns_color_scheme());
            match background::hold_color_authority() {
                Ok(Some(hold)) => {
                    tracing::info!(
                        "[noctalia-v5] engine colour authority yielded for the apply \
                         (theme.policy -> off); flip observed in {:.0} ms",
                        hold.observed_in().as_millis()
                    );
                    authority_guard = Some(hold);
                }
                Ok(None) => {
                    tracing::debug!(
                        "[noctalia-v5] no engine colour authority to yield; behaving \
                         exactly as today"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "[noctalia-v5] cannot yield engine colour authority ({}); \
                         proceeding as today",
                        e
                    );
                }
            }
        } else {
            owner_snapshot = None;
        }
        let path: Option<PathBuf> = match theme_video.as_ref() {
            Some(crate::theme_media::ThemeVideoSource::Path(path)) => Some(path.clone()),
            _ => None,
        };
        if let Some(path) = path {
            if background::hand_off_path(&path) {
                animated_applied = true;
                applied_video = Some(path.clone());
                tracing::info!(
                    "[noctalia-v5] Restored theme video via skwd: {}",
                    path.display()
                );
            } else {
                // D1: the video is NOT back — keep every fallback
                // route (manifest below, static restore after it)
                // instead of claiming success and deleting them.
                tracing::warn!(
                    "[noctalia-v5] Video delegation failed ({}); keeping fallback routes",
                    path.display()
                );
            }
        } else if let Some(crate::theme_media::ThemeVideoSource::Url {
            url,
            filename,
            sha256,
        }) = theme_video.as_ref()
        {
            // T3: an over-bound video was never copied. Hand its recorded url
            // to the mpvpaper backend through the SAME manifest the plugin
            // already plays. Best-effort: a failure only warns.
            match crate::providers::bg_info::ensure_url_manifest(
                &provider_dir,
                filename.as_deref(),
                url,
                sha256.as_deref(),
            ) {
                Ok(crate::providers::bg_info::UrlManifestOutcome::Written) => tracing::info!(
                    "[noctalia-v5] Wrote the theme video url into the mpvpaper manifest: {}",
                    url
                ),
                Ok(crate::providers::bg_info::UrlManifestOutcome::KeptExisting) => tracing::info!(
                    "[noctalia-v5] Kept the existing mpvpaper manifest; nothing written for the \
                     theme video url: {}",
                    url
                ),
                Err(e) => tracing::warn!(
                    "[noctalia-v5] Cannot hand the theme video url to mpvpaper ({}); \
                     the theme keeps its poster",
                    e
                ),
            }
        } else if painter_record.exists() {
            // Preserve today's honest warnings: the absolute record exists but
            // yields nothing usable (a dead path or an unparseable record).
            match recorded_absolute {
                Some(path) => tracing::warn!(
                    "[noctalia-v5] Painter video no longer on disk ({}); falling back",
                    path.display()
                ),
                None => {
                    tracing::warn!(
                        "[noctalia-v5] Cannot parse video.txt; falling back to manifest"
                    );
                }
            }
        }
        if animated_applied {
            // The exact video is back with its own manager: a stale mpvpaper
            // manifest must not bounce the plugin over it.
            remove_stale_artifact(&provider_dir.join(crate::providers::bg_info::MANIFEST_FILE));
        }
        // 1b. Legacy animated wallpaper (mpvpaper plugin manifest).
        //    Only when no exact painter video was restored. Refuses (never
        //    enables) while the plugin is disabled — see mpvpaper.rs.
        //    The hand-off to the OTHER backends goes through the
        //    `apply-background` router: this path names no backend. The
        //    up-front notification for a missing/disabled plugin happens
        //    inside the router, before the manifest leg runs, exactly as
        //    it did here.
        // T6: HOW the theme wants its video decides which backend can paint
        // it — an exact path goes to the wallpaper engine, a url to the plugin
        // through the manifest — so the notice must read the KIND, never a
        // single "wants a video" boolean.
        let video_want = match theme_video.as_ref() {
            Some(crate::theme_media::ThemeVideoSource::Path(_)) => {
                crate::providers::bg_info::VideoWant::ExactPath
            }
            Some(crate::theme_media::ThemeVideoSource::Url { .. }) => {
                crate::providers::bg_info::VideoWant::Url
            }
            None => crate::providers::bg_info::VideoWant::None,
        };
        let mut bg_request = background::ApplyBackgroundRequest {
            theme_dir: theme_dir.to_path_buf(),
            provider_dir,
            animated_applied,
            static_wallpaper,
            video_want,
        };
        let outcome = background::apply_animated(&bg_request);
        animated_applied = outcome.animated_applied;
        match outcome.leg {
            background::RoutedLeg::AlreadyApplied => {}
            background::RoutedLeg::ManifestApplied => {
                tracing::info!("[noctalia-v5] Animated wallpapers applied");
            }
            background::RoutedLeg::ManifestRefused(reason) => {
                tracing::warn!("[noctalia-v5] Animated wallpapers skipped: {}", reason);
            }
            background::RoutedLeg::Cleared => {
                tracing::info!("[noctalia-v5] Stopped running video wallpapers");
            }
            background::RoutedLeg::ClearFailed(reason) => {
                tracing::warn!("[noctalia-v5] clear-all warning: {}", reason);
            }
        }

        // 1c. Phase 2 — neutralize Noctalia's own static rotation. The
        //     settings.toml restore above re-armed `[wallpaper.automation]
        //     enabled = true` (every saved theme carries it), which would
        //     paint a stale image over the video every N seconds. Turn the
        //     LIVE flag off and reload — best-effort and idempotent, never
        //     failing the apply.
        if animated_applied && settings_restored {
            neutralize_noctalia_wallpaper_rotation();
        }

        // 1d. Phase 1 — pin Noctalia to the video POSTER. With a video on
        //     top the static restore below is skipped, so Noctalia would
        //     otherwise keep the stale image the restored settings.toml
        //     names. A frame of the video is the only honest thing for its
        //     static layer to show. Best-effort: no poster, no change.
        if animated_applied {
            if let Some(video) = applied_video.as_deref() {
                match crate::shell::gallery::thumbs::extract_video_frame(
                    video,
                    &crate::shell::gallery::thumbs::cache_dir(),
                ) {
                    Some(poster) => {
                        if let Some(poster) = poster.to_str() {
                            match noctalia_msg(&["msg", "wallpaper-set", "", poster]) {
                                Ok(_) => tracing::info!(
                                    "[noctalia-v5] Pinned Noctalia static wallpaper to the video poster: {}",
                                    poster
                                ),
                                Err(e) => tracing::warn!(
                                    "[noctalia-v5] Cannot pin Noctalia to the video poster: {}",
                                    e
                                ),
                            }
                        }
                    }
                    None => tracing::warn!(
                        "[noctalia-v5] Could not extract a poster frame from the video: {}",
                        video.display()
                    ),
                }
            }
        }

        // 2. Restore wallpaper FIRST — for wallpaper-derived schemes, setting
        //    the wallpaper triggers Noctalia's palette regeneration, so it
        //    must happen before color-scheme-set and templates-apply.
        //    SKIP when a video wallpaper was successfully applied: the mpvpaper
        //    plugin disables the static wallpaper while a video plays, so
        //    re-setting the static image here would cover the running video.
        //    If the animated apply failed, fall back to the static wallpaper.
        //
        //    The colour-authority yield already happened BEFORE the animated
        //    branch above (W4): one flip per apply, the engine held off
        //    through every hand-off in this phase. The guard — armed with
        //    the previous value — restores on every exit path unless the
        //    re-assert worker takes ownership.
        //
        //    T2 theme-first: when the theme carries its own packaged poster
        //    (`poster.txt` naming a file inside the theme), that file is
        //    resolved against the theme dir AT APPLY TIME and wins over the
        //    absolute record — so a theme copied to another machine still
        //    paints its own background even though the absolute wallpaper.txt
        //    path does not exist there. A missing, invalid or traversing
        //    record (or a declared file that is not on disk) falls back to
        //    the chain below, byte-for-byte as today.
        if !animated_applied {
            if let Some(poster) = theme_poster {
                let poster_arg = poster.to_string_lossy().to_string();
                noctalia_msg(&["msg", "wallpaper-set", "", &poster_arg])
                    .map_err(|e| format!("Cannot set wallpaper: {}", e))?;
                tracing::info!(
                    "[noctalia-v5] Restored theme-packaged poster: {}",
                    poster.display()
                );
                // Best-effort delegation to skwd-walld through the
                // `apply-background` router, same as the static record below:
                // the engine gets the SAME resolved absolute path.
                bg_request.static_wallpaper = Some(poster);
                background::hand_off_static(&bg_request);
            } else if wp_path.exists() {
                let wp = fs::read_to_string(&wp_path)
                    .map_err(|e| format!("Cannot read wallpaper.txt: {}", e))?;
                let wp = wp.trim();
                if !wp.is_empty() {
                    noctalia_msg(&["msg", "wallpaper-set", "", wp])
                        .map_err(|e| format!("Cannot set wallpaper: {}", e))?;
                    tracing::info!("[noctalia-v5] Restored wallpaper: {}", wp);
                    // Best-effort delegation to skwd-walld (agnostic, silent if absent)
                    // — reached through the `apply-background` router. The
                    // authoritative read above is what Noctalia just set: state it
                    // on the request so the router hands the engine the SAME path.
                    bg_request.static_wallpaper = Some(PathBuf::from(wp));
                    background::hand_off_static(&bg_request);
                }
            }
        }

        // 2. Restore color scheme
        // Tracks the custom palette actually restored below, so the
        // ownership re-assert at the end fires only for what was set.
        let mut custom_restored: Option<String> = None;
        if source_src.exists() {
            let raw = fs::read_to_string(&source_src)
                .map_err(|e| format!("Cannot read source.txt: {}", e))?;
            let scheme = raw.trim();
            let parts: Vec<&str> = scheme.splitn(2, ' ').collect();
            let source = parts.first().copied().unwrap_or("custom");
            let name = parts.get(1).copied().unwrap_or("");

            if !name.is_empty() && palette_src.exists() {
                // Custom or community palette — copy the saved palette file back
                // and restore as "custom" regardless of original source.
                let safe_name = name.replace('/', "_");

                if let Some(palettes_dir) = noctalia_config_dir().map(|d| d.join("palettes")) {
                    fs::create_dir_all(&palettes_dir)
                        .map_err(|e| format!("Cannot create palettes dir: {}", e))?;
                    let dst = palettes_dir.join(format!("{}.json", safe_name));
                    fs::copy(&palette_src, &dst)
                        .map_err(|e| format!("Cannot restore palette: {}", e))?;
                    tracing::info!("[noctalia-v5] Restored palette to {:?}", dst);

                    noctalia_msg(&["msg", "color-scheme-set", "custom", &safe_name])
                        .map_err(|e| format!("Cannot set color scheme: {}", e))?;
                    tracing::info!("[noctalia-v5] Set active scheme: custom {}", safe_name);
                    // The applied theme owns the colours: declare it (backend,
                    // theme, saved snapshot, palette name) so the central
                    // colour pipeline can follow the snapshot instead of the
                    // live palette. Best-effort: a failed write warns and
                    // never fails the apply; nothing reads it yet.
                    let theme_name = theme_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    let authority = crate::color_authority::ColorAuthority {
                        backend: self.id().to_string(),
                        theme: theme_name,
                        palette_file: palette_src.display().to_string(),
                        palette_name: safe_name.clone(),
                    };
                    if let Err(e) = crate::color_authority::write_descriptor(&authority) {
                        tracing::warn!(
                            "[noctalia-v5] Cannot write colour-authority descriptor: {}",
                            e
                        );
                    }
                    custom_restored = Some(safe_name);
                }
            } else if !name.is_empty() && matches!(source, "builtin" | "community") {
                // Builtin or community — no palette file to copy, restore directly.
                noctalia_msg(&["msg", "color-scheme-set", source, name])
                    .map_err(|e| format!("Cannot set color scheme: {}", e))?;
                tracing::info!("[noctalia-v5] Set active scheme: {} {}", source, name);
            } else {
                // Wallpaper scheme (or no named scheme): the theme carries no
                // palette snapshot, so it claims no colour authority — drop a
                // stale descriptor instead of writing one.
                if let Err(e) = crate::color_authority::clear_descriptor() {
                    tracing::warn!(
                        "[noctalia-v5] Cannot clear colour-authority descriptor: {}",
                        e
                    );
                }
            }
            // wallpaper scheme: already handled by wallpaper restoration above

            // 3. Apply templates so rendered files (noctalia-colors.conf, etc.)
            //    reflect the restored palette. color-scheme-set only persists
            //    the setting; templates-apply renders the actual output files.
            noctalia_msg(&["msg", "templates-apply"])
                .map_err(|e| format!("Cannot apply templates: {}", e))?;
            tracing::info!("[noctalia-v5] Templates applied");

            // 4. The keeper's wallpaper engine may own the color scheme (see
            //    `background::engine_owns_color_scheme`): it then regenerates its
            //    wallpaper palette asynchronously AFTER these synchronous
            //    steps and re-imposes `custom skwd-wall`. Only a restored
            //    CUSTOM palette qualifies for the re-assert — builtin,
            //    community and wallpaper schemes keep today's behaviour
            //    untouched. Off the UI thread, single-shot, bounded.
            //
            //    The ownership decision comes from the PRE-yield snapshot:
            //    the flip set theme.policy to "off", so re-evaluating here
            //    would deny the re-assert (the W2 trap). When the authority
            //    was yielded, the worker restores the previous value once
            //    the palette is verified (or at the bounded cap); the guard
            //    is disarmed ONLY when the worker takes ownership, so every
            //    other exit path (early error, no custom palette) still
            //    restores.
            if let Some(restored) = custom_restored {
                let owned = owner_snapshot.unwrap_or_else(background::engine_owns_color_scheme);
                if owned {
                    tracing::info!(
                        "[noctalia-v5] Wallpaper engine owns the color scheme; re-asserting '{}' off-thread",
                        restored
                    );
                    let previous = authority_guard.as_mut().and_then(|guard| guard.disarm());
                    spawn_custom_scheme_reassert(restored, previous);
                }
            }
        }

        // U7 + U9 + U16 of `odd/tasks/noctalia-layout-survives-the-apply.md`:
        // this is the LAST step of the apply — every write and reload the
        // apply performs itself (the settings restore, the D4 re-assert,
        // the wallpaper-rotation neutralize, `wallpaper-set`,
        // `color-scheme-set`, `templates-apply`) has already happened, so
        // the verify samples the state the apply actually leaves behind
        // (acceptance criterion 2) and no later synchronous step can
        // re-wipe behind it. U9: the verify itself runs OFF-THREAD on a
        // detached worker — the apply returns without waiting for it.
        // U16: the SPAWN happens in `spawn_duty`'s `Drop`, which fires on
        // this `Ok(())` AND on every `?` return above, so an Err from any
        // step still hands the duty on. Best-effort exactly like the D4
        // re-assert: a layout problem must never fail the theme apply.
        // One caveat, stated: the off-thread colour re-assert spawned
        // above and this verify worker are both asynchronous by design
        // and can still land after the apply returns.

        Ok(())
    }

    /// Re-assert this backend's colours for an applied theme (unit 1c2 of
    /// `odd/tasks/hve-capability-routing.md`): when the theme carries our
    /// colour snapshot and the live state drifted, put it back — copy +
    /// `color-scheme-set` + `templates-apply`, reusing the apply path's own
    /// steps. Read-only until the decision: anything unowned or already
    /// matching is `Ok(())` with no CLI write at all.
    fn reassert_colours(&self, theme_dir: &Path) -> Result<(), String> {
        let provider_dir = theme_dir.join("providers").join(self.id());
        let snapshot = match fs::read(provider_dir.join("palette.json")) {
            Ok(bytes) if !bytes.is_empty() => bytes,
            _ => return Ok(()),
        };
        let source = match fs::read_to_string(provider_dir.join("source.txt")) {
            Ok(text) => text,
            Err(_) => return Ok(()),
        };
        // Same parse as the apply path: `custom <name>` owns a snapshot,
        // anything else (builtin/community/wallpaper) carries no palette
        // file to put back.
        let mut parts = source.trim().splitn(2, ' ');
        let origin = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("").trim();
        if origin != "custom" || name.is_empty() {
            return Ok(());
        }
        // Same sanitization the apply path applies before the name touches
        // a file name or a CLI argument (`Command` args never see a shell).
        let safe_name = name.replace('/', "_");
        // While an apply holds the engine's colour authority off, the hold
        // is in flight and the re-assert worker owns the palette — a
        // watcher-driven re-assert now would fight it. Same held state the
        // apply path opens and clears, asked of the router; quiet by
        // design, nothing alarming.
        if background::color_authority_held() {
            tracing::debug!(
                "[noctalia-v5] colour authority yielded to the engine; skipping colour re-assert"
            );
            return Ok(());
        }
        let live_path = find_palette_path("custom", &safe_name);
        let live_palette = live_path
            .as_ref()
            .and_then(|path| fs::read(path).ok());
        let live_scheme = noctalia_msg(&["msg", "color-scheme-get"])
            .ok()
            .map(|out| out.trim().to_string());
        if !palette_needs_reassert(
            live_scheme.as_deref(),
            live_palette.as_deref(),
            &snapshot,
            &safe_name,
        ) {
            return Ok(());
        }
        let live_path =
            live_path.ok_or_else(|| "Cannot resolve live palette path".to_string())?;
        if let Some(parent) = live_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot create palettes dir: {}", e))?;
        }
        fs::write(&live_path, &snapshot)
            .map_err(|e| format!("Cannot restore palette: {}", e))?;
        noctalia_msg(&["msg", "color-scheme-set", "custom", &safe_name])
            .map_err(|e| format!("Cannot set color scheme: {}", e))?;
        noctalia_msg(&["msg", "templates-apply"])
            .map_err(|e| format!("Cannot apply templates: {}", e))?;
        tracing::info!("[noctalia-v5] Colours re-asserted: custom {}", safe_name);
        Ok(())
    }

    /// Rebuild the colour-authority descriptor from this theme's OWN saved
    /// state (capability-routing R2, the upgrade window: a theme applied
    /// before the descriptor existed otherwise declares no authority until
    /// the next apply). Same parse and same construction as the apply path
    /// above — but purely declarative: nothing is copied into a live
    /// palette, no CLI runs, and `None` comes back whenever the state that
    /// would have to be declared is missing.
    fn derive_colour_authority(
        &self,
        theme_dir: &Path,
    ) -> Option<crate::color_authority::ColorAuthority> {
        let provider_dir = theme_dir.join("providers").join(self.id());
        let palette_src = provider_dir.join("palette.json");
        let raw = fs::read_to_string(provider_dir.join("source.txt")).ok()?;
        let mut parts = raw.trim().splitn(2, ' ');
        let origin = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("").trim();
        // Same rule as the apply path: only a custom palette carries a
        // snapshot we can point at; builtin/community/wallpaper schemes
        // claim no colour authority.
        if origin != "custom" || name.is_empty() {
            return None;
        }
        // An empty snapshot is no snapshot — the same guard
        // `reassert_colours` applies before it would write bytes back.
        match fs::read(&palette_src) {
            Ok(bytes) if !bytes.is_empty() => {}
            _ => return None,
        }
        let theme_name = theme_dir.file_name()?.to_str()?.to_string();
        Some(crate::color_authority::ColorAuthority {
            backend: self.id().to_string(),
            theme: theme_name,
            palette_file: palette_src.display().to_string(),
            // Same sanitization the apply path applies before the name
            // touches a file name or a CLI argument.
            palette_name: name.replace('/', "_"),
        })
    }

    /// The backend contract's `deletable-artefacts` (capability-routing
    /// Phase 3): a custom palette restored outside the theme dir lives in
    /// this backend's own `palettes/` directory and is SHARED by every
    /// theme recording the same name — so on delete the core may remove it
    /// only while no surviving theme still declares it. The provider
    /// supplies its own knowledge only (its record file, its parse, its
    /// live path); the reference decision, the fail-open scan and the
    /// "applied theme keeps its live palette" rule stay the core's.
    /// Nothing declared — and therefore nothing deleted — when the record
    /// is missing or unreadable, the scheme is not `custom`, or the config
    /// dir cannot be resolved.
    fn deletable_artifacts(&self, theme_dir: &Path) -> Vec<DeletableArtifact> {
        let provider_dir = theme_dir.join("providers").join(self.id());
        let Ok(text) = fs::read_to_string(provider_dir.join("source.txt")) else {
            return Vec::new();
        };
        let Some(safe_name) = parse_custom_palette_name(&text) else {
            return Vec::new();
        };
        let Some(config_dir) = noctalia_config_dir() else {
            return Vec::new();
        };
        vec![DeletableArtifact {
            path: config_dir.join("palettes").join(format!("{safe_name}.json")),
            reference_key: safe_name,
        }]
    }

    fn post_apply(&self, theme_name: &str) -> Result<(), String> {
        // No reload needed — v5 hot-reloads automatically.
        // Just a notification.
        match std::process::Command::new("notify-send")
            .arg("--app-name=HVE")
            .arg(format!("🎨 Tema '{}' aplicado", theme_name))
            .arg(format!(
                "Theme \"{}\" applied — Noctalia v5 colors, settings, wallpaper restored",
                theme_name,
            ))
            .output()
        {
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("[noctalia-v5] notify-send not available: {}", e);
            }
        }

        Ok(())
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    /// The DND spelling the shell adapter owns: "on"/"off" are the only
    /// statuses Noctalia reports, and they map to the CLI's `true`/`false`.
    /// A mapping typo here would silently break DND restore for the shipped
    /// setup, so it stays pinned by a test.
    #[test]
    fn test_noctalia_dnd_bool_maps_status_words() {
        assert_eq!(dnd_bool_arg("on"), Some("true"));
        assert_eq!(dnd_bool_arg("off"), Some("false"));
        assert_eq!(dnd_bool_arg(""), None);
        assert_eq!(dnd_bool_arg("garbage"), None);
    }

    #[test]
    fn test_noctalia_v5_provider_new() {
        let provider = NoctaliaV5Provider::new();
        assert_eq!(provider.id(), "noctalia-v5");
        assert_eq!(provider.shell(), "noctalia-v5");
    }

    #[test]
    fn test_noctalia_v5_provider_display_name_key() {
        let provider = NoctaliaV5Provider::new();
        assert_eq!(provider.display_name_key(), "themes.provider.noctalia_v5");
    }

    #[test]
    fn test_noctalia_v5_provider_icon() {
        let provider = NoctaliaV5Provider::new();
        assert_eq!(provider.icon(), "◈");
    }

    #[test]
    fn test_noctalia_v5_provider_capabilities() {
        let provider = NoctaliaV5Provider::new();
        let caps = provider.capabilities();
        assert!(caps.contains(ProviderCapabilities::COLORS));
        assert!(caps.contains(ProviderCapabilities::WALLPAPERS));
    }

    /// The `deletable-artefacts` declaration (capability-routing Phase 3):
    /// the v5 provider declares the custom palette file it restores OUTSIDE
    /// the theme dir — its own `palettes/` layout, its own record parse.
    /// A missing record or a non-custom scheme declares nothing, so the
    /// core deletes nothing; `/` is sanitized exactly as the apply path
    /// sanitizes it before the name touches a file name.
    #[test]
    fn deletable_artifacts_declares_the_custom_palette_only() {
        let _env = crate::test_utils::TempEnv::new();
        let old_noctalia = std::env::var("HVE_NOCTALIA_CONFIG").ok();
        let noct_home = TempDir::new().unwrap();
        std::env::set_var("HVE_NOCTALIA_CONFIG", noct_home.path());

        let provider = NoctaliaV5Provider::new();
        let theme = TempDir::new().unwrap();
        let prov_dir = theme.path().join("providers").join("noctalia-v5");
        std::fs::create_dir_all(&prov_dir).unwrap();

        // Missing record: nothing declared, therefore nothing deleted.
        assert!(provider.deletable_artifacts(theme.path()).is_empty());

        // Non-custom scheme: the backend owns no palette file for it.
        std::fs::write(prov_dir.join("source.txt"), "builtin Ocean\n").unwrap();
        assert!(provider.deletable_artifacts(theme.path()).is_empty());

        // Custom scheme: the live shared-palette path + the reference key.
        std::fs::write(prov_dir.join("source.txt"), "custom Joker/Theme\n").unwrap();
        assert_eq!(
            provider.deletable_artifacts(theme.path()),
            vec![DeletableArtifact {
                path: noct_home.path().join("palettes").join("Joker_Theme.json"),
                reference_key: "Joker_Theme".to_string(),
            }]
        );

        match old_noctalia {
            Some(v) => std::env::set_var("HVE_NOCTALIA_CONFIG", v),
            None => std::env::remove_var("HVE_NOCTALIA_CONFIG"),
        }
    }

    /// Stale-artifact deletes must not swallow failures: silencing
    /// remove_file hides a read-only dir or I/O error and the save looks
    /// clean while the wrong background record survives. Comment lines are
    /// stripped first so a commented-out delete cannot satisfy this
    /// (block/trailing comments are not stripped). Needles are built with
    /// concat() so this test's own source — it lives in the file it
    /// inspects — can never satisfy them.
    #[test]
    fn stale_artifact_deletes_log_failures_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let silent_delete = ["let _", " = fs::remove", "_file"].concat();
        assert!(
            !code.contains(&silent_delete),
            "stale deletes must log their failure instead of silencing it"
        );
        let logged_wallpaper =
            ["remove_stale_artifact", "(&provider_dir.join(\"wallpaper"].concat();
        let logged_manifest =
            ["remove_stale_artifact", "(&provider_dir.join(crate"].concat();
        let warns = ["Could not remove ", "stale"].concat();
        assert!(
            code.contains(&logged_wallpaper) && code.contains(&logged_manifest),
            "both stale deletes (wallpaper.txt, mpvpaper manifest) must go through the logging helper"
        );
        assert!(
            code.contains(&warns),
            "stale deletes must warn with the path and reason"
        );
    }

    #[test]
    #[serial]
    fn test_noctalia_v5_save_creates_provider_dir() {
        // Save requires noctalia running — we only check that creating the
        // provider directory is attempted. TempDir cleans up
        // automatically on drop (no leftover state between runs).
        //
        // WHY #[serial]: `save()` now snapshots the mpvpaper plugin state
        // through the same `noctalia msg` IPC the stub harnesses shadow onto
        // the process-global PATH. Unserialized, this smoke test could slip
        // an extra probe into `PluginProbeStub`'s exact-count log (and it
        // already races `ColorStub`'s `color-scheme-get`). The serial group
        // keeps it from overlapping any stub.
        let provider = NoctaliaV5Provider::new();
        let dir = TempDir::new().unwrap();

        // No assert on result — it can be Ok (noctalia running) or
        // Err (noctalia not available). We only check that it does not panic.
        let _result = provider.save(dir.path());
    }

    #[test]
    fn theme_facts_report_palette_colors_and_the_palette_name() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("source.txt"), "custom JokerTheme\n").unwrap();
        fs::write(
            tmp.path().join("palette.json"),
            r##"{"dark":{"mPrimary":"#2ec436","mSecondary":"#fb9e0f","mTertiary":"#9d00ff","mSurface":"#0C1017"},"light":{}}"##,
        )
        .unwrap();

        let facts = theme_facts_from_records(tmp.path());
        let rows: Vec<(String, String)> = facts
            .iter()
            .filter(|f| !f.value.is_empty())
            .map(|f| (f.label.clone(), f.value.clone()))
            .collect();
        let swatches: Vec<String> = facts.iter().flat_map(|f| f.swatches.clone()).collect();
        assert_eq!(
            rows,
            vec![
                ("Palette source".to_string(), "custom".to_string()),
                ("Palette".to_string(), "JokerTheme".to_string()),
            ],
            "the color-carrying fact has no label/value row: the core drops it"
        );
        assert_eq!(swatches, vec!["#2ec436", "#fb9e0f", "#9d00ff", "#0C1017"]);
    }

    #[test]
    fn theme_facts_report_the_palette_name_when_the_colors_live_elsewhere() {
        // A wallpaper-derived palette has no color file: the panel shows the
        // name and invents nothing.
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("source.txt"), "wallpaper vibrant").unwrap();

        let facts = theme_facts_from_records(tmp.path());
        let rows: Vec<(String, String)> = facts
            .iter()
            .filter(|f| !f.value.is_empty())
            .map(|f| (f.label.clone(), f.value.clone()))
            .collect();
        let swatches: Vec<String> = facts.iter().flat_map(|f| f.swatches.clone()).collect();
        assert!(swatches.is_empty(), "never invent colors for a wallpaper palette");
        assert_eq!(
            rows,
            vec![
                ("Palette source".to_string(), "wallpaper".to_string()),
                ("Palette".to_string(), "vibrant".to_string()),
            ]
        );
    }

    #[test]
    fn theme_facts_fall_back_to_a_saved_custom_scheme() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("colors.json"),
            r##"{"mPrimary":"#ff0000","mSurface":"#000000"}"##,
        )
        .unwrap();

        let facts = theme_facts_from_records(tmp.path());
        let swatches: Vec<String> = facts.iter().flat_map(|f| f.swatches.clone()).collect();
        assert_eq!(swatches, vec!["#ff0000", "#000000"]);
    }

    #[test]
    fn theme_facts_are_empty_for_an_unreadable_record() {
        let tmp = TempDir::new().unwrap();
        assert!(theme_facts_from_records(tmp.path()).is_empty());
        fs::write(tmp.path().join("palette.json"), "{broken").unwrap();
        let facts = theme_facts_from_records(tmp.path());
        assert!(
            facts.iter().all(|f| f.swatches.is_empty()),
            "a broken palette file must not invent colors"
        );
    }

    #[test]
    fn test_find_palette_path_custom() {
        let path = find_palette_path("custom", "JokerTheme");
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.ends_with("JokerTheme.json"));
        assert!(path.to_string_lossy().contains("palettes"));
    }

    /// The exact live case: Noctalia stores a community palette under its
    /// URL-ENCODED name (`Catppuccin%20Mocha%20Sapphire.json`), so the RAW
    /// name HVE used to build never resolved — the save warned and silently
    /// kept the palette an earlier save had left behind.
    #[test]
    fn community_lookup_resolves_the_encoded_file_name() {
        let _env = crate::test_utils::TempEnv::new();
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let dir = home
            .join(".local")
            .join("state")
            .join("noctalia")
            .join("community-palettes");
        std::fs::create_dir_all(&dir).unwrap();
        let encoded = dir.join("Catppuccin%20Mocha%20Sapphire.json");
        std::fs::write(&encoded, br##"{"dark":{"mPrimary":"#74c7ec"}}"##).unwrap();

        assert_eq!(
            find_palette_path("community", "Catppuccin Mocha Sapphire").as_deref(),
            Some(encoded.as_path()),
            "the community palette must resolve to the file Noctalia actually wrote"
        );
    }

    /// The encoding Noctalia applies to its community palette file names:
    /// spaces become `%20`, the `.json` extension is kept, and an
    /// already-encoded name is NOT encoded twice (`%` stays `%`, so `%20`
    /// never becomes `%2520`).
    #[test]
    fn community_palette_file_name_encodes_spaces_only() {
        assert_eq!(
            community_palette_file_name("Catppuccin Mocha Sapphire"),
            "Catppuccin%20Mocha%20Sapphire.json"
        );
        assert_eq!(
            community_palette_file_name("Ayu"),
            "Ayu.json",
            "a name without spaces keeps its file name apart from the extension"
        );
        assert_eq!(
            community_palette_file_name(""),
            "",
            "no name, no palette file; the capture decision rejects it first"
        );
        assert_eq!(
            community_palette_file_name("Catppuccin%20Mocha"),
            "Catppuccin%20Mocha.json",
            "an already-encoded name must not be encoded twice"
        );
    }

    /// The raw name stays a valid lookup: a palette stored WITHOUT the
    /// encoding (whatever wrote it) still resolves, so the encoded form
    /// adds a convention instead of replacing one.
    #[test]
    fn community_lookup_falls_back_to_the_raw_file_name() {
        let _env = crate::test_utils::TempEnv::new();
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let dir = home
            .join(".local")
            .join("state")
            .join("noctalia")
            .join("community-palettes");
        std::fs::create_dir_all(&dir).unwrap();
        let raw = dir.join("Ayu Green.json");
        std::fs::write(&raw, br##"{"dark":{"mPrimary":"#b8bb26"}}"##).unwrap();

        assert_eq!(
            find_palette_path("community", "Ayu Green").as_deref(),
            Some(raw.as_path()),
            "a raw-named community palette must keep resolving"
        );
    }

    /// The invariant behind the capture decision: a theme's `palette.json`
    /// exists if and only if its declared source resolves to a palette. A
    /// save is a CAPTURE of the live truth, so a theme whose declared
    /// palette cannot be resolved must not keep the file an EARLIER save
    /// left behind — that leftover is what painted the JokerTheme colours
    /// under a theme declaring a Catppuccin palette.
    #[test]
    fn palette_capture_action_removes_the_stale_palette_when_nothing_resolves() {
        assert_eq!(
            palette_capture_action("community", "Catppuccin Mocha Sapphire", None),
            PaletteCaptureAction::RemoveStale,
            "an unresolvable community palette must not leave a foreign palette behind"
        );
        assert_eq!(
            palette_capture_action("custom", "JokerTheme", None),
            PaletteCaptureAction::RemoveStale,
            "an unresolvable custom palette must not leave a foreign palette behind"
        );
    }

    #[test]
    fn palette_capture_action_copies_a_resolved_palette() {
        let resolved = PathBuf::from("/palettes/Catppuccin%20Mocha%20Sapphire.json");
        assert_eq!(
            palette_capture_action("community", "Catppuccin Mocha Sapphire", Some(&resolved)),
            PaletteCaptureAction::Copy
        );
        assert_eq!(
            palette_capture_action("custom", "JokerTheme", Some(&resolved)),
            PaletteCaptureAction::Copy
        );
    }

    #[test]
    fn palette_capture_action_rejects_an_empty_name() {
        let resolved = PathBuf::from("/palettes/.json");
        assert_eq!(
            palette_capture_action("custom", "", None),
            PaletteCaptureAction::RemoveStale
        );
        assert_eq!(
            palette_capture_action("custom", "", Some(&resolved)),
            PaletteCaptureAction::RemoveStale,
            "an empty name declares no palette, whatever path was resolved"
        );
    }

    #[test]
    fn palette_capture_action_rejects_a_source_that_owns_no_palette() {
        let resolved = PathBuf::from("/palettes/whatever.json");
        assert_eq!(
            palette_capture_action("wallpaper", "vibrant", None),
            PaletteCaptureAction::RemoveStale
        );
        assert_eq!(
            palette_capture_action("builtin", "Kanagawa", None),
            PaletteCaptureAction::RemoveStale
        );
        assert_eq!(
            palette_capture_action("wallpaper", "vibrant", Some(&resolved)),
            PaletteCaptureAction::RemoveStale,
            "wallpaper/builtin own no palette file, resolved path or not"
        );
    }

    #[test]
    fn test_find_palette_path_community() {
        let path = find_palette_path("community", "Ayu Green");
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.to_string_lossy().contains("community-palettes"));
    }

    #[test]
    fn test_find_palette_path_builtin_returns_none() {
        let path = find_palette_path("builtin", "Kanagawa");
        assert!(path.is_none());
    }

    #[test]
    fn test_find_palette_path_wallpaper_returns_none() {
        let path = find_palette_path("wallpaper", "");
        assert!(path.is_none());
    }

    /// D1: a failed delegation must not claim the video is back. The apply
    /// path may only treat the painter video as restored on a real success;
    /// otherwise the manifest must survive and the static restore must run.
    /// The hand-off itself moved behind the `apply-background` router, so
    /// the gate is asserted on the routed call. Comment lines are stripped
    /// first so a commented-out gate cannot satisfy this. Needles are
    /// built with concat() so this test's own source — it lives in the
    /// file it inspects — can never satisfy them.
    #[test]
    fn v5_apply_claims_video_only_on_real_delegation_success_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let gated = ["if background::hand_off", "_path(&path)"].concat();
        assert!(
            code.contains(&gated),
            "apply must gate the painter-video success on the routed hand-off result"
        );
        let unconditional = ["hand_off_path(&path);\n", "                        animated_applied = true"].concat();
        assert!(
            !code.contains(&unconditional),
            "apply must not set animated_applied unconditionally after delegating"
        );
    }

    /// D4: the `[plugins] enabled` list in settings.toml is the machine's
    /// plugin state. The keeper's live file has the plugin disabled while
    /// every saved theme carries it enabled — the parser must tell both
    /// apart, including multi-line arrays and commented lines.
    #[test]
    fn settings_toml_parser_detects_mpvpaper_state() {
        let enabled = "[plugins]\nenabled = [ \"kenn/keybind-cheatsheet\", \"noctalia/mpvpaper\", \"yuuto/arch-updater\" ]\n";
        assert!(mpvpaper_enabled_in_settings(enabled));
        let disabled = "[plugins]\nenabled = [ \"yuuto/arch-updater\" ]\n";
        assert!(!mpvpaper_enabled_in_settings(disabled));
        let multiline = "[plugins]\nenabled = [\n  \"kenn/keybind-cheatsheet\",\n  \"noctalia/mpvpaper\",\n]\n";
        assert!(mpvpaper_enabled_in_settings(multiline));
        let commented = "[plugins]\n# enabled = [ \"noctalia/mpvpaper\" ]\nenabled = [ \"yuuto/arch-updater\" ]\n";
        assert!(!mpvpaper_enabled_in_settings(commented));
        let other_section = "[other]\nenabled = [ \"noctalia/mpvpaper\" ]\n[plugins]\nenabled = [ \"yuuto/arch-updater\" ]\n";
        assert!(!mpvpaper_enabled_in_settings(other_section));
        assert!(!mpvpaper_enabled_in_settings(""));
    }

    /// G6: the capture gate must not fold a parse failure into "provably
    /// disabled". This reader answers `Some(true)`/`Some(false)` ONLY when
    /// an explicit `[plugins] enabled` entry was found; anything it cannot
    /// locate — missing section, missing key, format drift, other section —
    /// is `None`, which the capture arm reads as unknown and resolves
    /// through the IPC probe. The boolean reader keeps its pinned
    /// false-for-unparseable contract for apply's re-assert.
    #[test]
    fn mpvpaper_state_in_settings_is_tri_state() {
        let enabled = "[plugins]\nenabled = [ \"kenn/keybind-cheatsheet\", \"noctalia/mpvpaper\", \"yuuto/arch-updater\" ]\n";
        assert_eq!(mpvpaper_state_in_settings(enabled), Some(true));
        let disabled = "[plugins]\nenabled = [ \"yuuto/arch-updater\" ]\n";
        assert_eq!(mpvpaper_state_in_settings(disabled), Some(false));
        let multiline = "[plugins]\nenabled = [\n  \"kenn/keybind-cheatsheet\",\n  \"noctalia/mpvpaper\",\n]\n";
        assert_eq!(mpvpaper_state_in_settings(multiline), Some(true));
        let commented = "[plugins]\n# enabled = [ \"noctalia/mpvpaper\" ]\nenabled = [ \"yuuto/arch-updater\" ]\n";
        assert_eq!(mpvpaper_state_in_settings(commented), Some(false));
        // Nothing to parse: unknown, never disabled.
        assert_eq!(mpvpaper_state_in_settings(""), None);
        assert_eq!(
            mpvpaper_state_in_settings("[other]\nenabled = [ \"noctalia/mpvpaper\" ]\n"),
            None,
            "another section's enabled key is not the plugin state"
        );
        assert_eq!(
            mpvpaper_state_in_settings("[plugins]\nfoo = 1\n"),
            None,
            "a [plugins] section without an enabled key is unknown"
        );
        assert_eq!(
            mpvpaper_state_in_settings("[plugins]\nactive = [ \"noctalia/mpvpaper\" ]\n"),
            None,
            "format drift (a key the line parser cannot read) must be unknown"
        );
        assert!(
            !mpvpaper_enabled_in_settings("[plugins]\nfoo = 1\n"),
            "the boolean reader keeps its false-for-unparseable contract"
        );
    }

    // ── U1: settings.toml must carry no personal path ────────────────────

    /// The hook lines name a script that lives on the machine that saved the
    /// theme — someone else's script, in fact. They are machine-only, so the
    /// whole line goes, and the section header survives its emptied body.
    #[test]
    fn sanitize_drops_machine_only_hook_lines_and_keeps_the_header() {
        let root = TempDir::new().unwrap();
        let theme = root.path().join("Cars");
        let raw = "[hooks]\n\
                   colors_changed = \"/home/ximo/.local/share/sddm/themes/SilentDinamic/sddm-sync.sh\"\n\
                   shutdown = \"/home/ximo/.local/share/sddm/themes/SilentDinamic/sddm-sync.sh\"\n\
                   started = \"/home/ximo/.local/share/sddm/themes/SilentDinamic/sddm-sync.sh\"\n\
                   \n\
                   [shell]\n\
                   enabled = true\n";

        let (clean, report) = sanitize_settings_toml(raw, &theme);

        assert_eq!(
            clean, "[hooks]\n\n[shell]\nenabled = true\n",
            "a dropped line must vanish whole, leaving the header and the rest"
        );
        assert_eq!(report.dropped, 3);
        assert_eq!(
            report.dropped_keys,
            vec!["colors_changed", "shutdown", "started"]
        );
        assert_eq!(report.rewritten, 0);
        assert!(!report.is_empty());
    }

    /// An absolute value that points INSIDE the theme is not machine-only: it
    /// names an asset the theme carries, so it is rewritten to the
    /// theme-relative path instead of being dropped.
    #[test]
    fn sanitize_rewrites_an_absolute_path_inside_the_theme() {
        let root = TempDir::new().unwrap();
        let theme = root.path().join("Cars");
        let inside = theme.join("media").join("poster.jpg");
        let raw = format!("[wallpaper.default]\n    path = \"{}\"\n", inside.display());

        let (clean, report) = sanitize_settings_toml(&raw, &theme);

        assert_eq!(
            clean, "[wallpaper.default]\n    path = \"media/poster.jpg\"\n",
            "an in-theme asset must stay reachable as a theme-relative path"
        );
        assert_eq!(report.rewritten, 1);
        assert_eq!(report.dropped, 0);
    }

    /// A rewrite touches the value token only: indentation, key, spacing and
    /// any trailing comment survive byte-for-byte.
    #[test]
    fn sanitize_preserves_indentation_and_a_trailing_comment_on_a_rewrite() {
        let root = TempDir::new().unwrap();
        let theme = root.path().join("Cars");
        let inside = theme.join("media").join("poster.jpg");
        let raw = format!(
            "[wallpaper.monitors.DP-3]\n    path = \"{}\" # packaged\n",
            inside.display()
        );

        let (clean, report) = sanitize_settings_toml(&raw, &theme);

        assert_eq!(
            clean,
            "[wallpaper.monitors.DP-3]\n    path = \"media/poster.jpg\" # packaged\n"
        );
        assert_eq!(report.rewritten, 1);
    }

    /// Relative values, non-path values, arrays, comments and blank lines are
    /// theme content, not machine state: they are returned untouched.
    #[test]
    fn sanitize_leaves_relative_non_path_arrays_comments_and_blanks_untouched() {
        let root = TempDir::new().unwrap();
        let theme = root.path().join("Cars");
        let raw = "# a comment\n\
                   \n\
                   [plugins]\n\
                   enabled = [ \"kenn/keybind-cheatsheet\", \"noctalia/mpvpaper\" ]\n\
                   \n\
                   [wallpaper]\n\
                   directory = \"media/wallpapers\"\n\
                   interval_seconds = 600\n\
                   mode = \"dark\"\n";

        let (clean, report) = sanitize_settings_toml(raw, &theme);

        assert_eq!(clean, raw, "every non-path byte must stay identical");
        assert!(
            report.is_empty(),
            "nothing to report when nothing was rewritten or dropped: {report:?}"
        );
    }

    /// A path record can be written unquoted (a bare token). It is still an
    /// absolute machine path, so the line goes.
    #[test]
    fn sanitize_drops_a_bare_absolute_value() {
        let root = TempDir::new().unwrap();
        let theme = root.path().join("Cars");
        let raw = "[wallpaper]\ndirectory = /home/ximo/Pictures/Wallpapers/Cars\ntransition_on_startup = true\n";

        let (clean, report) = sanitize_settings_toml(raw, &theme);

        assert_eq!(clean, "[wallpaper]\ntransition_on_startup = true\n");
        assert_eq!(report.dropped, 1);
        assert_eq!(report.dropped_keys, vec!["directory"]);
    }

    /// The theme dir itself is not an asset inside it: rewriting it would
    /// leave an empty value, so it is dropped like any other machine path.
    #[test]
    fn sanitize_drops_a_value_that_is_the_theme_dir_itself() {
        let root = TempDir::new().unwrap();
        let theme = root.path().join("Cars");
        let raw = format!("[wallpaper]\ndirectory = \"{}\"\n", theme.display());

        let (clean, report) = sanitize_settings_toml(&raw, &theme);

        assert_eq!(clean, "[wallpaper]\n");
        assert_eq!(report.dropped, 1);
        assert_eq!(report.rewritten, 0);
    }

    /// U2: the absolute background records must not survive a save. They name
    /// THIS machine; the packaged equivalent is what a target machine
    /// resolves. A record WITH a packaged equivalent is dropped quietly
    /// (info), one WITHOUT is still dropped and the loss is reported (warn) —
    /// a foreign absolute path is the defect being removed.
    #[test]
    fn drop_absolute_background_records_removes_them_and_reports_the_loss() {
        let root = TempDir::new().unwrap();
        let provider = root.path().join("providers").join("noctalia-v5");
        fs::create_dir_all(&provider).unwrap();
        fs::write(provider.join("wallpaper.txt"), "/home/ximo/Pictures/a.png").unwrap();
        fs::write(provider.join("video.txt"), "path=/home/ximo/v.mp4\n").unwrap();

        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        drop_absolute_background_records(&provider, true, false);

        assert!(
            !provider.join("wallpaper.txt").exists(),
            "the absolute wallpaper record must go"
        );
        assert!(
            !provider.join("video.txt").exists(),
            "the absolute video record must go"
        );
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("INFO") && text.contains("wallpaper.txt"),
            "a dropped record WITH a packaged equivalent must be reported at info, got: {text}"
        );
        assert!(
            text.contains("WARN") && text.contains("video.txt"),
            "a dropped record WITHOUT a packaged equivalent must warn, got: {text}"
        );
    }

    /// A provider dir that carries no absolute record stays quiet and is not
    /// touched.
    #[test]
    fn drop_absolute_background_records_is_a_no_op_without_records() {
        let root = TempDir::new().unwrap();
        let provider = root.path().join("providers").join("noctalia-v5");
        fs::create_dir_all(&provider).unwrap();
        fs::write(provider.join("poster.txt"), "poster=media/poster.jpg\n").unwrap();

        drop_absolute_background_records(&provider, true, true);

        assert_eq!(
            fs::read_to_string(provider.join("poster.txt")).unwrap(),
            "poster=media/poster.jpg\n",
            "the packaged record is what travels and must stay"
        );
    }

    /// The shipped settings.toml shape, end to end: the keeper's own file
    /// loses every machine path and keeps every other byte. This is the
    /// contract's own observed key set (hooks, wallpaper, screenshot, plugin
    /// video dir, per-monitor paths).
    #[test]
    fn sanitize_cleans_the_observed_shipped_key_set() {
        let root = TempDir::new().unwrap();
        let theme = root.path().join("Animation");
        let inside = theme.join("media").join("poster.jpg");
        let raw = format!(
            "[hooks]\n\
             started = \"/home/ximo/.local/share/sddm/themes/SilentDinamic/sddm-sync.sh\"\n\
             \n\
             [plugin_settings.\"noctalia/mpvpaper\"]\n\
             video_directory = \"/home/ximo/Pictures/LiveWallpapers\"\n\
             \n\
             [shell.screenshot]\n\
             directory = \"/home/ximo/Pictures/Screenshots\"\n\
             \n\
             [wallpaper]\n\
             directory = \"/home/ximo/Pictures/Wallpapers/Cars\"\n\
             \n\
             [wallpaper.default]\n\
             path = \"{}\"\n\
             \n\
             [wallpaper.monitors.DP-3]\n\
             path = \"/home/ximo/.cache/hve/thumbs/c628e057fad10c72-frame.png\"\n",
            inside.display()
        );

        let (clean, report) = sanitize_settings_toml(&raw, &theme);

        assert!(!clean.contains("/home/"), "no personal path may survive: {clean}");
        assert!(
            clean.contains("path = \"media/poster.jpg\""),
            "the in-theme asset must stay reachable: {clean}"
        );
        assert!(clean.contains("[hooks]\n") && clean.contains("[wallpaper.monitors.DP-3]\n"));
        assert_eq!(report.dropped, 5);
        assert_eq!(report.rewritten, 1);
        assert_eq!(
            report.dropped_keys,
            vec!["started", "video_directory", "directory", "directory", "path"]
        );
    }

    /// Phase 2: the live settings.toml editor flips
    /// `[wallpaper.automation] enabled` to false while preserving every
    /// other byte, is idempotent on an already-off flag, and never touches
    /// another section's `enabled`.
    #[test]
    fn wallpaper_automation_editor_flips_true_to_false_preserving_the_rest() {
        let raw = "[wallpaper]\n\
                   directory = \"/home/u/Pictures/Wallpapers/Cars\"\n\
                   \n\
                   [wallpaper.automation]\n\
                   enabled = true\n\
                   interval = 600\n\
                   \n\
                   [bar]\n\
                   enabled = true\n";
        let edited = disable_wallpaper_automation(raw)
            .expect("an armed rotation must be turned off");
        let expected = raw.replacen("enabled = true", "enabled = false", 1);
        assert_eq!(
            edited, expected,
            "only the automation flag may change; every other byte must survive"
        );
        // Idempotent: an already-off flag reports nothing to change.
        assert_eq!(
            disable_wallpaper_automation(&edited),
            None,
            "an already-off flag must be a no-op"
        );
        // A missing section is a no-op, never a guess.
        assert_eq!(
            disable_wallpaper_automation("[bar]\nenabled = true\n"),
            None,
            "the editor must not touch a flag outside [wallpaper.automation]"
        );
        // Section presence is what keeps the caller's log honest.
        assert!(has_wallpaper_automation_section(raw));
        assert!(!has_wallpaper_automation_section("[bar]\nenabled = true\n"));
    }

    /// D4: the re-assert mapping is the proof a theme carrying the plugin
    /// enabled cannot leave it enabled — a disabled pre-apply state maps
    /// to disabling it again after the reload.
    #[test]
    fn plugin_reassert_keeps_pre_apply_state() {
        assert_eq!(plugin_reassert_action(Some(false)), PluginReassert::Disable);
        assert_eq!(plugin_reassert_action(Some(true)), PluginReassert::Enable);
        assert_eq!(plugin_reassert_action(None), PluginReassert::Leave);
    }

    /// G3: the live mpvpaper manifest may be copied into a theme ONLY when
    /// the `noctalia/mpvpaper` plugin can actually paint. A provably
    /// DISABLED plugin owns nothing, so its live assignments file is stale
    /// (it names the previous video) and the gallery ranks a live
    /// assignment above the theme's own static wallpaper — copying it would
    /// hijack the theme's preview and re-apply. `None` (unreadable) is
    /// never disabled, so the legacy capture stays.
    ///
    /// The reused tri-state is the same one apply reads: `Some(false)` is a
    /// positive "disabled", not "leave alone".
    #[test]
    fn video_manifest_capture_requires_an_enabled_or_unknown_plugin() {
        assert!(
            should_capture_video_manifest(true, Some(true)),
            "an enabled plugin can paint: the legacy capture must stay"
        );
        assert!(
            should_capture_video_manifest(true, None),
            "an unreadable state is unknown, never disabled: the legacy capture must stay"
        );
        assert!(
            !should_capture_video_manifest(true, Some(false)),
            "a provably disabled plugin must never let its stale manifest into a theme"
        );
        assert!(
            !should_capture_video_manifest(false, Some(true)),
            "an exact static/video arm captures no manifest even when the plugin is enabled"
        );
        assert!(!should_capture_video_manifest(false, Some(false)));
        assert!(!should_capture_video_manifest(false, None));
        assert_eq!(
            plugin_reassert_action(Some(false)),
            PluginReassert::Disable,
            "the same tri-state must keep reading Some(false) as a defined disabled"
        );
    }

    /// What the stub answers the `plugins list` probe with.
    enum ProbeAnswer {
        /// The probe prints this line and exits 0.
        Line(&'static str),
        /// The probe succeeds but prints no matching line.
        Empty,
        /// The probe logs the call, then the CLI fails outright.
        Fail,
    }

    /// Hermetic `noctalia msg plugins list` sandbox — the SAME mechanism as
    /// `ColorStub` above and `ProbeSandbox` in `background.rs`: `TempEnv`
    /// takes the shared environment lock and redirects `HOME` (so the live
    /// `settings.toml` the primary signal reads lands inside the
    /// sandbox), the stub bin dir is prepended to `PATH`, and `Drop`
    /// restores the original `PATH` while that lock is still held. The
    /// stub logs ONLY `plugins list` invocations, so a concurrent
    /// suite-mate's unrelated IPC (the shell wallpaper test spawns
    /// `noctalia` without the env lock) cannot pollute the probe log.
    struct PluginProbeStub {
        /// Restored first: `Drop` puts PATH back before any field goes.
        old_path: Option<String>,
        /// Every `msg plugins list` the probe sent, in order.
        log: PathBuf,
        /// Sandbox HOME: the primary signal reads
        /// `<home>/.local/state/noctalia/settings.toml`.
        home: PathBuf,
        /// Lives on PATH until `old_path` is restored above.
        _bin: TempDir,
        /// Shared env lock, released last.
        _env: crate::test_utils::TempEnv,
    }

    impl PluginProbeStub {
        fn new(answer: ProbeAnswer) -> Self {
            let env = crate::test_utils::TempEnv::new();
            let bin = tempfile::tempdir().expect("stub bin dir");
            let log = bin.path().join("probes.log");
            let logging = format!("printf 'msg plugins list\\n' >> '{}'\n", log.display());
            let script = match answer {
                ProbeAnswer::Line(line) => format!(
                    "#!/bin/sh\n\
                     if [ \"$1\" = \"msg\" ] && [ \"$2\" = \"plugins\" ] && [ \"$3\" = \"list\" ]; then\n\
                     {logging}printf '%s' '{line}'\n\
                     fi\nexit 0\n"
                ),
                ProbeAnswer::Empty => format!(
                    "#!/bin/sh\n\
                     if [ \"$1\" = \"msg\" ] && [ \"$2\" = \"plugins\" ] && [ \"$3\" = \"list\" ]; then\n\
                     {logging}fi\nexit 0\n"
                ),
                ProbeAnswer::Fail => format!(
                    "#!/bin/sh\n\
                     if [ \"$1\" = \"msg\" ] && [ \"$2\" = \"plugins\" ] && [ \"$3\" = \"list\" ]; then\n\
                     {logging}echo 'no daemon' >&2\nexit 4\n\
                     fi\nexit 0\n"
                ),
            };
            let stub = bin.path().join("noctalia");
            std::fs::write(&stub, script).expect("stub script");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
                    .expect("stub must be executable");
            }
            let home = PathBuf::from(std::env::var("HOME").expect("TempEnv must set HOME"));
            let old_path = std::env::var("PATH").ok();
            let new_path = match &old_path {
                Some(prev) => format!("{}:{}", bin.path().display(), prev),
                None => bin.path().display().to_string(),
            };
            std::env::set_var("PATH", &new_path);
            Self {
                old_path,
                log,
                home,
                _bin: bin,
                _env: env,
            }
        }

        /// Write the live `settings.toml` under the sandbox HOME so the
        /// primary signal of `live_mpvpaper_plugin_enabled` answers.
        fn write_settings(&self, body: &str) {
            let dir = self.home.join(".local").join("state").join("noctalia");
            std::fs::create_dir_all(&dir).expect("live state dir");
            std::fs::write(dir.join("settings.toml"), body).expect("live settings.toml");
        }

        /// The recorded `msg plugins list` invocations, in order.
        fn probes(&self) -> Vec<String> {
            std::fs::read_to_string(&self.log)
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }
    }

    impl Drop for PluginProbeStub {
        fn drop(&mut self) {
            match &self.old_path {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
        }
    }

    /// D4: with no readable live settings.toml the snapshot falls back to
    /// the `plugins list` IPC — a matching `enabled` line reads
    /// `Some(true)`, a successful answer without one reads `Some(false)`,
    /// and each answers with exactly ONE probe.
    #[test]
    #[serial]
    fn live_snapshot_falls_back_to_the_plugins_list_ipc() {
        let stub = PluginProbeStub::new(ProbeAnswer::Line("noctalia/mpvpaper enabled"));
        assert_eq!(
            live_mpvpaper_plugin_enabled(),
            Some(true),
            "an `enabled` line for the plugin must snapshot as enabled"
        );
        assert_eq!(
            stub.probes(),
            vec!["msg plugins list"],
            "the fallback must send exactly one probe"
        );
        drop(stub);

        let stub = PluginProbeStub::new(ProbeAnswer::Empty);
        assert_eq!(
            live_mpvpaper_plugin_enabled(),
            Some(false),
            "a successful probe without a matching line must snapshot as disabled"
        );
        assert_eq!(stub.probes(), vec!["msg plugins list"]);
    }

    /// D4: a failing IPC is "unknown", never "disabled" — the caller then
    /// leaves the reloaded state alone instead of disabling a plugin it
    /// could not read.
    #[test]
    #[serial]
    fn live_snapshot_is_none_when_the_ipc_fails() {
        let stub = PluginProbeStub::new(ProbeAnswer::Fail);
        assert_eq!(
            live_mpvpaper_plugin_enabled(),
            None,
            "an IPC failure must read as unknown, never as disabled"
        );
        assert_eq!(stub.probes(), vec!["msg plugins list"]);
    }

    /// D4: the live settings.toml is the PRIMARY signal — it wins over an
    /// IPC answer that says the opposite, and it short-circuits the probe
    /// entirely (no IPC at all), even when that IPC would fail.
    #[test]
    #[serial]
    fn live_snapshot_prefers_the_live_settings_toml() {
        let stub = PluginProbeStub::new(ProbeAnswer::Line("noctalia/mpvpaper enabled"));
        stub.write_settings("[plugins]\nenabled = [ \"yuuto/arch-updater\" ]\n");
        assert_eq!(
            live_mpvpaper_plugin_enabled(),
            Some(false),
            "the live settings.toml must win over a contradicting IPC answer"
        );
        assert!(
            stub.probes().is_empty(),
            "a readable settings.toml must not trigger any IPC"
        );
        drop(stub);

        let stub = PluginProbeStub::new(ProbeAnswer::Fail);
        stub.write_settings("[plugins]\nenabled = [ \"noctalia/mpvpaper\" ]\n");
        assert_eq!(
            live_mpvpaper_plugin_enabled(),
            Some(true),
            "the primary signal must win even over a failing IPC"
        );
        assert!(stub.probes().is_empty(), "no IPC while the primary answers");
    }

    /// G6: the capture-specific snapshot must consult the IPC when the
    /// local settings.toml cannot name an explicit plugin state — a parse
    /// failure must never mean "disabled" on the capture arm (a future
    /// format drift would silently stop capturing while the plugin paints).
    /// Only an explicit local `[plugins] enabled` entry short-circuits the
    /// probe, exactly as `live_mpvpaper_plugin_enabled` does for apply.
    #[test]
    #[serial]
    fn capture_snapshot_falls_back_to_ipc_when_local_state_is_unknown() {
        // Explicit local disabled: suppress, and never ask the IPC.
        let stub = PluginProbeStub::new(ProbeAnswer::Line("noctalia/mpvpaper enabled"));
        stub.write_settings("[plugins]\nenabled = [ \"yuuto/arch-updater\" ]\n");
        assert_eq!(
            mpvpaper_can_paint_for_capture(),
            Some(false),
            "an explicit local disabled entry must read as provably disabled"
        );
        assert!(stub.probes().is_empty(), "a parseable local state must not probe");
        drop(stub);

        // Explicit local enabled: capture, and never ask the IPC.
        let stub = PluginProbeStub::new(ProbeAnswer::Fail);
        stub.write_settings("[plugins]\nenabled = [ \"noctalia/mpvpaper\" ]\n");
        assert_eq!(
            mpvpaper_can_paint_for_capture(),
            Some(true),
            "an explicit local enabled entry must read as enabled even when the IPC would fail"
        );
        assert!(stub.probes().is_empty(), "a parseable local state must not probe");
        drop(stub);

        // No explicit local state: the IPC decides, never an assumed disabled.
        let stub = PluginProbeStub::new(ProbeAnswer::Line("noctalia/mpvpaper enabled"));
        stub.write_settings("[plugins]\nfoo = 1\n");
        assert_eq!(
            mpvpaper_can_paint_for_capture(),
            Some(true),
            "an unparseable local state must fall back to the IPC, not read as disabled"
        );
        assert_eq!(stub.probes(), vec!["msg plugins list"]);
        drop(stub);

        // IPC answers without the plugin: provably disabled.
        let stub = PluginProbeStub::new(ProbeAnswer::Empty);
        stub.write_settings("[other]\nenabled = [ \"noctalia/mpvpaper\" ]\n");
        assert_eq!(
            mpvpaper_can_paint_for_capture(),
            Some(false),
            "an IPC answer without the plugin is a definite disabled"
        );
        assert_eq!(stub.probes(), vec!["msg plugins list"]);
        drop(stub);

        // Both signals fail: unknown, never disabled (the caller captures).
        let stub = PluginProbeStub::new(ProbeAnswer::Fail);
        stub.write_settings("[other]\nenabled = [ \"noctalia/mpvpaper\" ]\n");
        assert_eq!(
            mpvpaper_can_paint_for_capture(),
            None,
            "a local miss plus a failing IPC is unknown, never disabled"
        );
        assert_eq!(stub.probes(), vec!["msg plugins list"]);
        drop(stub);

        // No settings file at all: also the IPC's call.
        let stub = PluginProbeStub::new(ProbeAnswer::Line("noctalia/mpvpaper enabled"));
        assert_eq!(mpvpaper_can_paint_for_capture(), Some(true));
        assert_eq!(stub.probes(), vec!["msg plugins list"]);
    }

    /// The duplication this unit breaks: the apply-time snapshot must
    /// reach `plugins list` ONLY through the neutral seam's tri-state
    /// probe — a second parse here would be a second implementation to
    /// keep in sync. Comment lines are stripped first so a commented-out
    /// parse cannot satisfy it. Needles are built with concat() so this
    /// test's own source — it lives in the file it inspects — can never
    /// contain them.
    #[test]
    fn live_snapshot_probes_through_the_seam_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let parse = ["plugins\", \"list"].concat();
        assert!(
            !code.contains(&parse),
            "noctalia.rs must not parse `plugins list` itself — the probe belongs \
             to the neutral seam (found: {parse})"
        );
        let start = code
            .find("fn live_mpvpaper_plugin_enabled(")
            .expect("the snapshot function must exist");
        let brace_at = start + code[start..].find('{').expect("it must have a body");
        let rest = &code[brace_at..];
        let body = &rest[..rest.find("\n}").expect("the body must close")];
        assert!(
            body.contains("plugin_enabled_probe(") && body.contains("MPVPAPER_PLUGIN_ID"),
            "the fallback must call the seam probe with the seam's plugin id; body was:\n{body}"
        );
        assert!(
            body.contains("leaving reloaded state alone"),
            "the unknown-state warning must keep its wording; body was:\n{body}"
        );
    }

    /// D4: applying a theme restores settings.toml verbatim and runs
    /// config-reload, which would re-enable a plugin the user disabled.
    /// Apply must therefore snapshot the pre-apply enabled state BEFORE
    /// overwriting the live settings and re-assert it AFTER the reload —
    /// without touching the rest of the settings restore. Comment lines
    /// are stripped first so a commented-out snapshot cannot satisfy this.
    /// Needles are built with concat() so this test's own source — it
    /// lives in the file it inspects — can never satisfy them.
    #[test]
    fn v5_apply_preserves_mpvpaper_enabled_state_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let snapshot = ["plugin_", "was_enabled"].concat();
        assert!(
            code.contains(&snapshot),
            "apply must snapshot the pre-apply mpvpaper enabled state"
        );
        let snapshot_at = code.find(&snapshot).expect("snapshot must exist");
        let restore_at = code
            .find("Cannot restore settings.toml")
            .expect("apply must still restore settings.toml");
        assert!(
            snapshot_at < restore_at,
            "the snapshot must happen before the live settings are overwritten"
        );
        let reload_at = code
            .find("config-reload")
            .expect("apply must still reload the config");
        let reassert = ["plugins\", \"dis", "able\""].concat();
        let reassert_at = code[reload_at..]
            .find(&reassert)
            .expect("apply must re-assert the plugin state after the reload");
        let _ = reassert_at;
    }

    /// G2/G3: a painter-identified video must be recorded exactly (kind +
    /// path + who painted it) and restored through the manager in charge
    /// (`skwd-helm apply`, reached through the `apply-background` router),
    /// never by bouncing the mpvpaper plugin. Comment lines are stripped
    /// first so a commented-out call cannot satisfy this. Needles are
    /// built with concat() so this test's own source — it lives in the
    /// file it inspects — can never satisfy them.
    #[test]
    fn v5_apply_restores_painter_video_through_skwd_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let video_record = ["vid", "eo.txt"].concat();
        let router = ["background::hand_off", "_path(&path)"].concat();
        assert!(
            code.contains(&video_record),
            "noctalia.rs apply must restore the painter-identified video record"
        );
        assert!(
            code.contains(&router),
            "noctalia.rs apply must route the recorded video through the \
             apply-background router to the skwd engine"
        );
    }

    /// The chain this unit breaks: `noctalia.rs` must reach the skwd
    /// engine and the mpvpaper plugin ONLY through the `apply-background`
    /// router — never by naming their helpers or APIs. The provider's own
    /// knowledge stays (settings parser, colour-authority seams, the
    /// third-party plugin id as data). Comment lines are stripped first
    /// so a commented-out call cannot satisfy this. Needles are built
    /// with concat() so this test's own source — it lives in the file it
    /// inspects — can never contain them.
    #[test]
    fn v5_apply_routes_backgrounds_through_the_router_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in [
            ["delegate_to_", "skwd_walld"].concat(),
            ["run_delegate_", "apply"].concat(),
            ["skwd_wall_", "socket_path"].concat(),
            ["is_video_", "path"].concat(),
            ["providers::", "mpvpaper::"].concat(),
        ] {
            assert!(
                !code.contains(&needle),
                "noctalia.rs must not name `{}` — the hand-off belongs to the \
                 apply-background router (capability routing: a backend never \
                 calls another backend)",
                needle
            );
        }
        let routed_manifest = ["background::apply_", "animated"].concat();
        assert!(
            code.contains(&routed_manifest),
            "noctalia.rs apply must reach the manifest leg through the router"
        );
        let routed_path = ["background::hand_off", "_path("].concat();
        assert!(
            code.contains(&routed_path),
            "noctalia.rs must reach the path hand-off through the router"
        );
    }

    /// The chain THIS unit breaks: the colour-authority yield (W2/W4) —
    /// flipping the engine's `theme.policy` off around one apply and
    /// putting it back — configures the skwd engine's own config file. A
    /// backend never configures another backend, so `noctalia.rs` must
    /// name NO part of that mechanism: not `skwd_policy`, not the config
    /// resolver, not the yield, not the restore, not the marker, not the
    /// guard. The capability is reached through the `apply-background`
    /// router instead. The scan is cut at the trailing test module AND has
    /// comment lines stripped first, so neither this test's own source nor
    /// a commented-out call can satisfy (or fail) an assertion.
    #[test]
    fn v5_apply_reaches_the_colour_authority_yield_through_the_router() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let production: String = src
            .split("mod tests")
            .next()
            .expect("the production body comes first")
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // Old, forbidden coupling: the provider configuring the engine.
        for needle in [
            "skwd_policy",
            "YieldGuard",
            "yield_color_authority",
            "restore_color_authority",
            "marker_path",
        ] {
            assert!(
                !production.contains(needle),
                "noctalia.rs must not name `{}` — the colour-authority yield \
                 configures the skwd engine, and a backend never configures \
                 another backend: hold/release is a capability the \
                 apply-background router owns (found in the production body)",
                needle
            );
        }
        // New, required route: the router owns hold, release, the held
        // query and the ownership read.
        for needle in [
            "background::hold_color_authority(",
            "background::ColourAuthorityHold",
            "background::release_color_authority(",
            "background::color_authority_held()",
            "background::engine_owns_color_scheme()",
        ] {
            assert!(
                production.contains(needle),
                "noctalia.rs must reach the colour-authority capability through \
                 the apply-background router: `{}` is missing from the \
                 production body",
                needle
            );
        }
    }

    // ── skwd color-authority re-assert tests ──
    //
    // WHY these exist: when the keeper's wallpaper engine owns the color
    // scheme (skwd-wall-v2 config: theme.policy == "wallpaper" and/or
    // noctalia.themeMode == "follow"), it regenerates the wallpaper palette
    // asynchronously AFTER HVE's synchronous apply and re-imposes
    // `custom skwd-wall`, silently discarding the theme's custom palette.
    // HVE must detect that ownership and re-assert once, bounded, off the
    // UI thread — never a watcher war.

    const OWNER_JSON: &str =
        r#"{"theme": {"policy": "wallpaper"}, "noctalia": {"themeMode": "follow"}}"#;

    /// Variant WITHOUT `noctalia.themeMode == "follow"`: the only thing
    /// that made ownership true was `theme.policy == "wallpaper"`, so after
    /// the yield flip the config reads `off` and a POST-yield ownership
    /// re-evaluation answers FALSE — the exact W2 trap. The re-assert must
    /// be decided from the PRE-yield snapshot alone, or the palette is
    /// never re-asserted (and the flash returns, just later).
    const OWNER_TRAP_JSON: &str =
        r#"{"theme": {"policy": "wallpaper"}, "noctalia": {"themeMode": "manual"}}"#;

    /// Hermetic harness: a stub `noctalia` shadowed onto PATH plus a
    /// stub skwd-wall config path.
    ///
    /// One deliberate wrinkle: PATH shadowing is process-global, so a
    /// concurrent suite-mate that runs `noctalia msg color-scheme-get`
    /// could slip one extra get line into this log. The v5 save test used
    /// to be that suite-mate; it is now `#[serial]` because `save()` also
    /// snapshots the plugin through this same global PATH. Get counts stay
    /// LOWER BOUNDS defensively, and nothing else in the suite issues
    /// `color-scheme-set` or `templates-apply`, so set/template counts
    /// stay EXACT.
    ///
    /// The stub records every invocation in `<state>/log` and answers
    /// `color-scheme-get` from `<state>/scheme`. `NOCTALIA_YIELD_AFTER`
    /// controls how many `color-scheme-set` calls the keeper daemon
    /// "overrides" (ignores) before a set sticks — the async race, bottled.
    struct ColorStub {
        _env: crate::test_utils::TempEnv,
        _tmp: TempDir,
        state_dir: PathBuf,
        skwd_cfg: PathBuf,
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl ColorStub {
        fn new(scheme: &str, yield_after: u32, skwd_config: Option<&str>, fast: bool) -> Self {
            let env = crate::test_utils::TempEnv::new();
            let tmp = TempDir::new().unwrap();
            let keys = [
                "PATH",
                "SKWD_WALL_V2_CONFIG",
                "HVE_NOCTALIA_CONFIG",
                "HVE_REASSERT_PROFILE",
                "NOCTALIA_STUB_STATE",
                "NOCTALIA_YIELD_AFTER",
                "XDG_RUNTIME_DIR",
                "SKWD_WALL_V2_SOCK",
            ];
            let saved: Vec<(&'static str, Option<String>)> = keys
                .iter()
                .map(|k| (*k, std::env::var(k).ok()))
                .collect();
            let state_dir = tmp.path().join("state");
            let bin_dir = tmp.path().join("bin");
            std::fs::create_dir_all(&state_dir).unwrap();
            std::fs::create_dir_all(&bin_dir).unwrap();
            std::fs::write(state_dir.join("scheme"), scheme).unwrap();
            std::fs::write(state_dir.join("log"), b"").unwrap();
            let stub = r#"#!/bin/sh
STATE="${NOCTALIA_STUB_STATE:?missing stub state}"
YIELD_AFTER="${NOCTALIA_YIELD_AFTER:-0}"
if [ "$1" = "msg" ] && [ "$2" = "color-scheme-get" ]; then
  printf 'color-scheme-get\n' >> "$STATE/log"
  cat "$STATE/scheme"
  exit 0
fi
if [ "$1" = "msg" ] && [ "$2" = "color-scheme-set" ]; then
  printf 'color-scheme-set %s %s\n' "$3" "$4" >> "$STATE/log"
  n=$(grep -c '^color-scheme-set' "$STATE/log")
  if [ "$n" -gt "$YIELD_AFTER" ]; then
    printf '%s %s\n' "$3" "$4" > "$STATE/scheme"
  fi
  exit 0
fi
printf '%s\n' "$*" >> "$STATE/log"
exit 0
"#;
            let noctalia = bin_dir.join("noctalia");
            std::fs::write(&noctalia, stub).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&noctalia, std::fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
            // Hermetic `notify-send`: the desktop toasts HVE raises are
            // recorded instead of landing on the keeper's screen. Both the
            // post-apply toast and the T6 video-backend notice land in the
            // same file, so tests assert on their content.
            let notify_send = bin_dir.join("notify-send");
            std::fs::write(
                &notify_send,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
                    state_dir.join("notify.log").display()
                ),
            )
            .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&notify_send, std::fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
            let orig_path = std::env::var("PATH").unwrap_or_default();
            std::env::set_var(
                "PATH",
                format!("{}:{}", bin_dir.display(), orig_path),
            );
            std::env::set_var("NOCTALIA_STUB_STATE", &state_dir);
            std::env::set_var("NOCTALIA_YIELD_AFTER", yield_after.to_string());
            let skwd_cfg = tmp.path().join("skwd-config.json");
            if let Some(content) = skwd_config {
                std::fs::write(&skwd_cfg, content).unwrap();
            }
            std::env::set_var("SKWD_WALL_V2_CONFIG", &skwd_cfg);
            std::env::remove_var("HVE_NOCTALIA_CONFIG");
            if fast {
                std::env::set_var("HVE_REASSERT_PROFILE", "fast");
            } else {
                std::env::remove_var("HVE_REASSERT_PROFILE");
            }
            // No daemon socket in tests: delegation stays a silent no-op.
            std::env::set_var("XDG_RUNTIME_DIR", tmp.path());
            std::env::remove_var("SKWD_WALL_V2_SOCK");
            Self {
                _env: env,
                _tmp: tmp,
                state_dir,
                skwd_cfg,
                saved,
            }
        }

        /// Theme carrying (or not) a custom palette; points the palette
        /// restore at a temp noctalia home.
        fn custom_theme(&self, source: &str, with_palette: bool) -> TempDir {
            let theme = TempDir::new().unwrap();
            let dir = theme.path().join("providers").join("noctalia-v5");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("source.txt"), source).unwrap();
            if with_palette {
                std::fs::write(dir.join("palette.json"), r#"{"palette":"joker"}"#).unwrap();
            }
            let noct_home = self._tmp.path().join("noct-home");
            std::fs::create_dir_all(&noct_home).unwrap();
            std::env::set_var("HVE_NOCTALIA_CONFIG", &noct_home);
            theme
        }

        /// Every `notify-send` invocation the apply raised, one per line.
        fn notified(&self) -> String {
            std::fs::read_to_string(self.state_dir.join("notify.log")).unwrap_or_default()
        }

        fn count(&self, needle: &str) -> usize {
            // Line shapes in the stub log: `color-scheme-set <src> <name>`
            // (set branch), bare `color-scheme-get` (get branch), and the
            // full argv `msg templates-apply` (generic fall-through branch).
            std::fs::read_to_string(self.state_dir.join("log"))
                .unwrap_or_default()
                .lines()
                .filter(|l| match needle {
                    "color-scheme-set" => l.starts_with("color-scheme-set "),
                    "templates-apply" => l.trim() == "msg templates-apply",
                    other => l.trim() == other,
                })
                .count()
        }

        fn sets(&self) -> usize {
            self.count("color-scheme-set")
        }

        fn gets(&self) -> usize {
            self.count("color-scheme-get")
        }

        fn templates(&self) -> usize {
            self.count("templates-apply")
        }

        fn poll(&self, needle: &str, want: usize, timeout: Duration) -> usize {
            let start = Instant::now();
            loop {
                let n = self.count(needle);
                if n >= want || start.elapsed() >= timeout {
                    return n;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    impl Drop for ColorStub {
        fn drop(&mut self) {
            // A detached re-assert worker reads PROCESS-GLOBAL state: `PATH`
            // to resolve this stub's `noctalia`, and `SKWD_WALL_V2_CONFIG` /
            // `HOME` to reach the engine config and the colour-authority
            // descriptor. Keep THIS sandbox (and the serial test's globals)
            // installed until every worker has finished, so a worker can
            // never outlive its test and leak a `color-scheme-set` into the
            // next test's stub log or write the next test's engine config.
            //
            // Bounded: the worker exits by itself at its attempt cap
            // (production worst case settle + cap ≈ 10 s), and the wait can
            // never hang the suite. A deadline that expires while a worker is
            // still alive is the one case the wait cannot fix — it must be
            // LOUD, because a surviving worker keeps reading this sandbox's
            // process-global state and would quietly pollute the next test
            // (exactly the failure this wait was added for). `panicking()`
            // keeps it from turning a failing test into an abort.
            let deadline = Instant::now() + Duration::from_secs(20);
            while !reassert_workers_idle() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            if !reassert_workers_idle() {
                let msg = "a detached colour re-assert worker outlived its 20 s deadline: \
                           its sandbox is about to be restored and the next test can be polluted";
                eprintln!("ColorStub::drop: {msg}");
                if !std::thread::panicking() {
                    panic!("ColorStub::drop: {msg}");
                }
            }
            for (k, v) in &self.saved {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    /// In-memory tracing sink so the cap test can assert the honest
    /// warning without a global subscriber (thread-local default only).
    #[derive(Clone, Default)]
    struct LogCapture {
        buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl std::io::Write for LogCapture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.buf.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
        type Writer = LogCapture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    #[serial]
    fn skwd_config_missing_or_broken_never_owns_scheme() {
        // No file at the configured path: HVE behaves exactly as today.
        let stub = ColorStub::new("custom skwd-wall", 99, None, true);
        assert!(
            !background::engine_owns_color_scheme(),
            "missing keeper config must not count as ownership"
        );
        // Garbage content: never guess ownership from what we cannot read.
        std::fs::write(&stub.skwd_cfg, b"{{{ not json").unwrap();
        assert!(
            !background::engine_owns_color_scheme(),
            "unparsable keeper config must not count as ownership"
        );
    }

    #[test]
    #[serial]
    fn skwd_config_owner_when_policy_wallpaper_or_follow_mode() {
        let stub = ColorStub::new("custom skwd-wall", 99, None, true);
        // Either signal alone makes the engine the color authority.
        std::fs::write(
            &stub.skwd_cfg,
            r#"{"theme": {"policy": "wallpaper"}, "noctalia": {"themeMode": "manual"}}"#,
        )
        .unwrap();
        assert!(
            background::engine_owns_color_scheme(),
            "theme.policy == wallpaper must count as ownership"
        );
        std::fs::write(
            &stub.skwd_cfg,
            r#"{"theme": {"policy": "manual"}, "noctalia": {"themeMode": "follow"}}"#,
        )
        .unwrap();
        assert!(
            background::engine_owns_color_scheme(),
            "noctalia.themeMode == follow must count as ownership"
        );
        std::fs::write(&stub.skwd_cfg, OWNER_JSON).unwrap();
        assert!(background::engine_owns_color_scheme());
        // Neither signal: not an owner, behaviour byte-identical to today.
        std::fs::write(
            &stub.skwd_cfg,
            r#"{"theme": {"policy": "manual"}, "noctalia": {"themeMode": "manual"}}"#,
        )
        .unwrap();
        assert!(
            !background::engine_owns_color_scheme(),
            "no ownership signal must behave exactly as today"
        );
        std::fs::write(&stub.skwd_cfg, r#"{}"#).unwrap();
        assert!(!background::engine_owns_color_scheme());
    }

    #[test]
    #[serial]
    fn custom_scheme_reassert_holds_first_check_without_action() {
        // Scheme already stuck: zero extra commands, just the honest log.
        let stub = ColorStub::new("custom JokerTheme", 0, Some(OWNER_JSON), true);
        let held = reassert_custom_scheme_blocking(
            "JokerTheme",
            Duration::from_millis(0),
            3,
            Duration::from_millis(10),
            Duration::from_secs(2),
        );
        assert!(held, "an already-stuck scheme must report held");
        assert_eq!(stub.sets(), 0, "held on first check must not re-issue set");
        assert!(stub.gets() >= 1, "held on first check needs its get");
    }

    #[test]
    #[serial]
    fn custom_scheme_reasserts_once_then_verifies() {
        // Daemon overrode the synchronous set, the re-assert sticks: exactly
        // one re-assert (one set + one templates-apply), then verified held.
        let stub = ColorStub::new("custom skwd-wall", 0, Some(OWNER_JSON), true);
        let held = reassert_custom_scheme_blocking(
            "JokerTheme",
            Duration::from_millis(0),
            3,
            Duration::from_millis(10),
            Duration::from_secs(2),
        );
        assert!(held, "scheme must be held after one re-assert");
        assert_eq!(stub.sets(), 1, "exactly one re-assert set expected");
        assert!(stub.gets() >= 2, "one check before and one verification after");
        assert_eq!(stub.templates(), 1, "each re-assert re-renders templates");
    }

    #[test]
    #[serial]
    fn custom_scheme_reassert_gives_up_bounded_when_owner_never_yields() {
        // The keeper never yields: attempts stay bounded and the warning
        // names the knob — no war, no silent loss.
        let stub = ColorStub::new("custom skwd-wall", 99, Some(OWNER_JSON), true);
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let start = Instant::now();
        let held = reassert_custom_scheme_blocking(
            "JokerTheme",
            Duration::from_millis(0),
            3,
            Duration::from_millis(20),
            Duration::from_secs(5),
        );
        let elapsed = start.elapsed();
        assert!(!held, "a never-yielding owner must report not-held");
        assert_eq!(stub.sets(), 3, "at most 3 re-assert sets");
        assert!(stub.gets() >= 4, "3 checks plus the final verdict get");
        assert!(
            elapsed < Duration::from_secs(2),
            "re-assert must be bounded, took {:?}",
            elapsed
        );
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("JokerTheme"),
            "warning must name the palette, got: {}",
            text
        );
        assert!(
            text.contains("theme.policy"),
            "warning must name the keeper knob, got: {}",
            text
        );
    }

    #[test]
    #[serial]
    fn v5_apply_custom_owner_reasserts_off_thread_without_blocking() {
        // Production timing: apply must return long before the settle
        // period ends, while the re-assert still lands afterwards.
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_JSON), false);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let start = Instant::now();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        let apply_elapsed = start.elapsed();
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert!(
            apply_elapsed < Duration::from_millis(1500),
            "apply must not block on the re-assert, took {:?}",
            apply_elapsed
        );
        // The re-asserted set lands AFTER apply returned (spawned thread).
        let sets = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        assert_eq!(sets, 2, "one sync set plus exactly one re-assert");
        // `templates-apply` is a SEPARATE subprocess issued right after the set
        // (production: set then templates), so the second one may not have
        // landed when the set line appears. Poll for it, like the single-shot
        // sibling test does, instead of asserting on the instant — under a
        // loaded parallel suite that gap widens and the direct read raced.
        let templates = stub.poll("templates-apply", 2, Duration::from_secs(10));
        assert_eq!(templates, 2, "templates re-rendered per set");
        // Let the final verifying get finish so no thread outlives the test
        // (it would otherwise run against a removed stub PATH).
        std::thread::sleep(Duration::from_millis(2500));
        assert_eq!(stub.sets(), 2, "re-assert held: no further sets");
    }

    #[test]
    #[serial]
    fn v5_apply_custom_owner_reasserts_single_shot_fast() {
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let sets = stub.poll("color-scheme-set", 2, Duration::from_secs(5));
        assert_eq!(sets, 2, "one sync set plus exactly one re-assert");
        let templates = stub.poll("templates-apply", 2, Duration::from_secs(5));
        assert_eq!(templates, 2, "templates re-rendered per set");
        // Single-shot: after the re-assert holds, nothing else may fire.
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(stub.sets(), 2, "no second re-assert: single-shot per apply");
        assert!(stub.gets() >= 2, "one re-assert check plus its verification");
    }

    #[test]
    #[serial]
    fn v5_apply_without_owner_adds_no_scheme_commands() {
        // Custom palette but no keeper ownership: byte-identical to today —
        // exactly the one synchronous set, no spawned follow-up.
        let stub = ColorStub::new("custom skwd-wall", 99, None, true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        // Fast profile would have re-asserted within ms if one were spawned.
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 1, "only the synchronous set, zero extra");
        assert_eq!(stub.templates(), 1, "only the synchronous templates-apply");
    }

    #[test]
    #[serial]
    fn v5_apply_non_custom_palette_ignores_owner() {
        // `wallpaper vibrant` has no saved palette: the owner's policy
        // behaviour is untouched — zero scheme commands even with an owner.
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("wallpaper vibrant", false);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 0, "non-custom palette must issue no set");
        assert_eq!(stub.templates(), 1, "templates-apply still runs once");
    }

    #[test]
    #[serial]
    fn v5_apply_owner_never_yields_gives_up_bounded() {
        // Owner that never yields: 1 sync set + 3 re-asserts, then silence —
        // the give-up that proves this cannot become a war.
        let stub = ColorStub::new("custom skwd-wall", 99, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let sets = stub.poll("color-scheme-set", 4, Duration::from_secs(5));
        assert_eq!(sets, 4, "one sync set plus three bounded re-asserts");
        let templates = stub.poll("templates-apply", 4, Duration::from_secs(5));
        assert_eq!(templates, 4, "templates re-rendered per set");
        // After the cap: no further attempts, ever.
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(stub.sets(), 4, "gave up at the cap: no war");
    }

    // ── W2: yield the engine's colour authority around the apply ──────
    //
    // WHY these exist: before W2, applying a theme showed the engine's
    // auto-derived colours for ~4 s — the keeper's engine owns the colour
    // scheme (`theme.policy == "wallpaper"`), regenerates the wallpaper
    // palette asynchronously AFTER HVE's synchronous apply steps, and
    // re-imposes it. W2 flips the policy to `off` BEFORE the wallpaper
    // hand-off and holds it through the apply; W1 extends that hold: while
    // the theme owns the palette (its colour-authority descriptor exists),
    // releasing never puts the engine's value back — the mute outlives the
    // apply. Only an apply that leaves NO owner (no descriptor) restores the
    // engine's own value, and any exit path before the worker hand-off still
    // restores when no owner was declared.

    #[test]
    #[serial]
    fn v5_apply_yields_before_handoff_and_holds_the_mute_while_the_theme_rules() {
        // (a) The ownership decision is taken BEFORE the flip: the fixture
        // is OWNER_TRAP_JSON (policy wallpaper, NO themeMode follow), so
        // after the flip the config reads `off` and re-evaluating ownership
        // there would answer FALSE — denying the very re-assert that must
        // run (the W2 trap). W1: once this apply writes the theme's
        // colour-authority descriptor the theme OWNS the palette, so the
        // worker verifies and re-asserts (2 sets) but its release must KEEP
        // the mute: `theme.policy` stays `off`, byte-for-byte, and the
        // crash marker stays pending.
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_TRAP_JSON), false);
        let theme = stub.custom_theme("custom JokerTheme", true);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let held = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert!(
            held.contains("\"policy\": \"off\""),
            "the authority must be held off through the apply, got: {}",
            held
        );
        // The worker verifies the palette (2 s settle) and re-asserts.
        let sets = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        assert_eq!(sets, 2, "one sync set plus exactly one re-assert");
        // Let the worker's release run, then prove it wrote nothing.
        std::thread::sleep(Duration::from_millis(1500));
        let final_cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            final_cfg, held,
            "the mute must survive the worker's release byte-for-byte"
        );
        assert!(
            !final_cfg.contains("\"policy\": \"wallpaper\""),
            "the engine's own value must NOT come back while the theme rules"
        );
        assert!(
            background::color_authority_held(),
            "the held mute keeps the crash marker"
        );
    }

    #[test]
    #[serial]
    fn v5_apply_non_custom_theme_restores_an_already_held_engine() {
        // W1 lifecycle end to end: a custom theme holds the mute past its
        // apply (descriptor present, marker pending). Re-applying a theme
        // that claims NO palette must hand the engine its own value back —
        // the mute must never get stuck on with no owner.
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_TRAP_JSON), true);
        let first = stub.custom_theme("custom JokerTheme", true);
        std::fs::write(
            first.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        let res = NoctaliaV5Provider::new().apply(first.path());
        assert!(res.is_ok(), "first apply must succeed: {:?}", res);
        // The worker verifies + re-asserts, then its release KEEPS the mute.
        let _ = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(500));
        let held = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert!(
            held.contains("\"policy\": \"off\""),
            "precondition: the first apply holds the mute, got: {}",
            held
        );
        assert!(
            background::color_authority_held(),
            "precondition: the crash marker is pending"
        );

        // Second apply: a non-custom theme (no palette) claims no authority.
        let second = stub.custom_theme("wallpaper vibrant", false);
        std::fs::write(
            second.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker4.png",
        )
        .unwrap();
        let res = NoctaliaV5Provider::new().apply(second.path());
        assert!(res.is_ok(), "second apply must succeed: {:?}", res);
        assert_eq!(
            std::fs::read_to_string(&stub.skwd_cfg).unwrap(),
            OWNER_TRAP_JSON,
            "a theme with no palette must give the engine its own value back"
        );
        assert!(
            !background::color_authority_held(),
            "the restore clears the pending marker"
        );
    }

    #[test]
    #[serial]
    fn v5_apply_early_error_after_yield_still_restores() {
        // (b) Early return: apply fails AFTER the yield (the palette target
        // dir cannot be created) — the guard's Drop must restore and log.
        let stub = ColorStub::new("custom skwd-wall", 99, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        // Break the palette target: a FILE where the palettes dir must be
        // created inside. `noctalia_config_dir()` honours the env var only
        // when the path EXISTS, so the env var must name the file itself —
        // `create_dir_all(<file>/palettes)` then fails deterministically
        // AFTER the wallpaper hand-off.
        let broken = std::env::temp_dir().join(format!("hve-noct-broken-{}", std::process::id()));
        std::fs::write(&broken, b"file, not a dir").unwrap();
        std::env::set_var("HVE_NOCTALIA_CONFIG", &broken);
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _tracing = tracing::subscriber::set_default(subscriber);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_err(), "the broken palette target must fail apply");
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, OWNER_JSON,
            "an early error must still restore the authority: {}",
            cfg
        );
        assert!(
            !background::color_authority_held(),
            "restore clears the marker"
        );
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("yield guard"),
            "the guard's scope-exit restore must be logged, got: {}",
            text
        );
        let _ = std::fs::remove_file(&broken);
    }

    #[test]
    #[serial]
    fn v5_apply_without_custom_palette_restores_synchronously() {
        // (b) No-palette branch: a wallpaper is handed over (yield fires)
        // but there is no custom palette to verify — no worker is spawned,
        // so the guard must restore synchronously at the end of apply.
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("wallpaper vibrant", false);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _tracing = tracing::subscriber::set_default(subscriber);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, OWNER_JSON,
            "the no-palette branch must restore synchronously: {}",
            cfg
        );
        assert!(!background::color_authority_held());
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("yield guard"),
            "the guard's scope-exit restore must be logged, got: {}",
            text
        );
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 0, "non-custom palette must issue no set");
    }

    #[test]
    #[serial]
    fn v5_apply_without_ownership_writes_nothing_and_behaves_as_today() {
        // (c) Missing engine config: no yield, nothing written anywhere,
        // and the scheme commands stay exactly today's.
        let stub = ColorStub::new("custom skwd-wall", 99, None, true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert!(
            !stub.skwd_cfg.exists(),
            "no config file must mean no yield and no write"
        );
        assert!(
            !background::color_authority_held(),
            "no yield must leave no marker"
        );
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 1, "only the synchronous set, zero extra");
        assert_eq!(stub.templates(), 1, "only the synchronous templates-apply");
    }

    #[test]
    #[serial]
    fn v5_apply_fixed_policy_writes_nothing_and_behaves_as_today() {
        // (c) `fixed` policy: no ownership, no yield — the engine config
        // bytes must stay exactly as they were, marker absent, re-assert
        // behaviour unchanged (no worker: no spawn without ownership).
        let fixed = r#"{"theme": {"policy": "fixed"}, "noctalia": {"themeMode": "manual"}}"#;
        let stub = ColorStub::new("custom skwd-wall", 99, Some(fixed), true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, fixed,
            "a fixed policy must leave the engine config byte-identical: {}",
            cfg
        );
        assert!(!background::color_authority_held());
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 1, "only the synchronous set, zero extra");
        assert_eq!(stub.templates(), 1, "only the synchronous templates-apply");
    }

    // ── W4: the yield covers the animated hand-off too ────────────────
    //
    // WHY these exist: W2 wired the colour-authority yield only into the
    // STATIC-wallpaper branch of apply. A theme's ANIMATED wallpaper
    // (video.txt -> skwd-helm apply) is handed over EARLIER in the same
    // function, before the yield point — so the engine still regenerated
    // its palette and the ~4 s flash remained (measured in the keeper's
    // live log: no yield line before the video hand-off, slow re-assert
    // after). W4 hoists the yield BEFORE the whole wallpaper-restoration
    // phase: one yield per apply, whichever branch hands the wallpaper
    // over first, with the observe wait between the flip and that first
    // hand-off.
    //
    // The ORDER is made observable, not just asserted: a stub `skwd-helm`
    // / `skwd-wall-v2` on PATH (with an existing socket file so the
    // delegation actually runs) records what the engine config said at
    // hand-off time — `delegation saw policy off` when the yield had
    // already flipped it, `delegation saw no yield` otherwise.

    /// Install stub `skwd-helm` / `skwd-wall-v2` binaries that record the
    /// engine-config state at hand-off time into the stub log and exit with
    /// the configured code (0 = the hand-off succeeded, 1 = it failed).
    fn install_delegation_stubs(bin_dir: &Path, exit_code: &str) {
        for bin in ["skwd-helm", "skwd-wall-v2"] {
            let script = format!(
                r#"#!/bin/sh
STATE="${{NOCTALIA_STUB_STATE:?missing stub state}}"
CFG="${{SKWD_WALL_V2_CONFIG:-}}"
if [ "$1" = "apply" ]; then
  printf '%s apply %s\n' "{bin}" "$2" >> "$STATE/log"
  if [ -n "$CFG" ] && [ -f "$CFG" ] && grep -q '"policy": "off"' "$CFG" 2>/dev/null; then
    printf 'delegation saw policy off\n' >> "$STATE/log"
  else
    printf 'delegation saw no yield\n' >> "$STATE/log"
  fi
  exit {exit_code}
fi
printf '%s\n' "$*" >> "$STATE/log"
exit 0
"#,
                bin = bin,
                exit_code = exit_code
            );
            let path = bin_dir.join(bin);
            std::fs::write(&path, script).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
    }

    /// Make the delegation path live for a stub harness: an existing socket
    /// file at `SKWD_WALL_V2_SOCK` (the delegation gate) plus stub binaries
    /// on the harness PATH. `exit_code` decides whether the simulated
    /// hand-off succeeds or fails.
    fn enable_delegation(stub: &ColorStub, exit_code: &str) {
        let sock = stub._tmp.path().join("wall.sock");
        std::fs::write(&sock, b"x").unwrap();
        std::env::set_var("SKWD_WALL_V2_SOCK", &sock);
        install_delegation_stubs(&stub._tmp.path().join("bin"), exit_code);
    }

    /// Count stub-log lines STARTING with `prefix` (the existing `count`
    /// helper matches exact lines, but the delegation lines carry paths).
    fn count_prefix(stub: &ColorStub, prefix: &str) -> usize {
        std::fs::read_to_string(stub.state_dir.join("log"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.trim_start().starts_with(prefix))
            .count()
    }

    /// A theme whose provider dir carries an ANIMATED painter record
    /// (`video.txt`) naming a real temp video file, plus a static
    /// wallpaper.txt (the static branch stays available when the animated
    /// hand-off fails).
    fn animated_theme(stub: &ColorStub) -> TempDir {
        let theme = stub.custom_theme("custom JokerTheme", true);
        let video = stub._tmp.path().join("loop.mp4");
        std::fs::write(&video, b"fake video").unwrap();
        let dir = theme.path().join("providers").join("noctalia-v5");
        std::fs::write(
            dir.join("video.txt"),
            format!(
                "path={}\npainter=mpvpaper\nnamespace=dp-3\npid=1234\n",
                video.display()
            ),
        )
        .unwrap();
        std::fs::write(dir.join("wallpaper.txt"), "/tmp/joker3.png").unwrap();
        theme
    }

    /// Phase 1: a video apply must hand Noctalia a POSTER frame via
    /// `wallpaper-set` so its own static layer matches the video instead
    /// of showing the stale image the restored settings.toml re-armed.
    /// The poster is the video's cached frame (the same artifact the
    /// gallery uses); the stale static Cars wallpaper must still NOT be
    /// set while the video plays.
    #[test]
    #[serial]
    fn v5_apply_video_pins_noctalia_to_the_poster_frame() {
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_JSON), true);
        enable_delegation(&stub, "0");
        let theme = animated_theme(&stub);
        let video = stub._tmp.path().join("loop.mp4");
        // Pre-seed the deterministic frame cache so no ffmpeg run is needed
        // (`extract_video_frame` short-circuits on an existing artifact).
        let poster = crate::shell::gallery::thumbs::cached_frame_path(&video);
        std::fs::create_dir_all(poster.parent().unwrap()).unwrap();
        std::fs::write(&poster, b"cached frame").unwrap();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let log = std::fs::read_to_string(stub.state_dir.join("log")).unwrap_or_default();
        let expected = format!("msg wallpaper-set  {}", poster.display());
        assert!(
            log.lines().any(|l| l == expected),
            "the video apply must pin Noctalia to the poster `{expected}`, log was:\n{log}"
        );
        assert!(
            !log.lines().any(|l| {
                l.starts_with("msg wallpaper-set") && !l.ends_with("-frame.png")
            }),
            "the stale static wallpaper must not be set while a video plays, log was:\n{log}"
        );
        // Let the re-assert worker finish before the stub PATH goes away.
        let _ = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(500));
    }

    /// Phase 1 boundary: a static apply sets its OWN static wallpaper and
    /// must never pin a video poster frame.
    #[test]
    #[serial]
    fn v5_apply_static_never_pins_a_poster_frame() {
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_JSON), true);
        enable_delegation(&stub, "0");
        let theme = stub.custom_theme("custom JokerTheme", true);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let log = std::fs::read_to_string(stub.state_dir.join("log")).unwrap_or_default();
        assert!(
            log.lines().any(|l| l == "msg wallpaper-set  /tmp/joker3.png"),
            "the static apply must set its own static wallpaper, log was:\n{log}"
        );
        assert!(
            !log.contains("-frame.png"),
            "a static apply must never pin a video poster, log was:\n{log}"
        );
        let _ = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(500));
    }

    // ── Theme packages T2: apply resolves the media from the theme ────
    //
    // WHY these exist: a theme is a package that travels. The absolute
    // records it carries can point at paths that do not exist on the
    // machine it was copied to; the packaged `media/poster.*` named by
    // `poster.txt` is resolved against the theme dir AT APPLY TIME, so the
    // background still resolves. Old themes (no record, no media) must keep
    // resolving exactly as before.

    /// A theme carrying a packaged poster (`media/poster.jpg` + the
    /// theme-relative `poster.txt`), with an optional absolute `wallpaper.txt`
    /// that may point at a path that does not exist here.
    fn theme_with_packaged_poster(stub: &ColorStub, wallpaper: Option<&str>) -> (TempDir, PathBuf) {
        let theme = stub.custom_theme("wallpaper vibrant", false);
        let dir = theme.path().join("providers").join("noctalia-v5");
        let media = theme.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        let poster = media.join("poster.jpg");
        std::fs::write(&poster, b"\xff\xd8\xffpackaged-poster").unwrap();
        crate::theme_media::write_poster_record(&dir, Path::new("media/poster.jpg")).unwrap();
        if let Some(wp) = wallpaper {
            std::fs::write(dir.join("wallpaper.txt"), wp).unwrap();
        }
        (theme, poster)
    }

    fn stub_log(stub: &ColorStub) -> String {
        std::fs::read_to_string(stub.state_dir.join("log")).unwrap_or_default()
    }

    /// The "other machine" proof: the absolute record points at a path that
    /// does NOT exist, but the packaged poster inside the theme does. Apply
    /// must hand the theme's OWN file to the shell (and the engine) instead
    /// of the dead absolute path.
    #[test]
    #[serial]
    fn v5_apply_prefers_the_packaged_poster_over_a_missing_absolute_path() {
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        enable_delegation(&stub, "0");
        let dead = "/nonexistent/other-machine/wall.jpg";
        let (theme, poster) = theme_with_packaged_poster(&stub, Some(dead));
        assert!(!Path::new(dead).exists(), "fixture must be a dead path");

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);

        let log = stub_log(&stub);
        let shell_line = format!("msg wallpaper-set  {}", poster.display());
        assert!(
            log.lines().any(|l| l == shell_line),
            "the theme's packaged poster must be handed to the shell, log was:\n{log}"
        );
        assert!(
            !log.contains(dead),
            "the dead absolute record must not be handed over, log was:\n{log}"
        );
        let engine_line = format!("skwd-wall-v2 apply {}", poster.display());
        let engine_line_alt = format!("skwd-helm apply {}", poster.display());
        assert!(
            log.lines().any(|l| l == engine_line || l == engine_line_alt),
            "the theme's packaged poster must be handed to the engine too, log was:\n{log}"
        );
        let _ = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(500));
    }

    /// A video theme drops `wallpaper.txt`, so with no video backend the
    /// apply used to end with NO static background. The packaged poster must
    /// become that static fallback.
    #[test]
    #[serial]
    fn v5_apply_video_without_backend_paints_the_poster_statically() {
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        // No delegation socket: no engine can paint the theme's video.
        let theme = stub.custom_theme("wallpaper vibrant", false);
        let dir = theme.path().join("providers").join("noctalia-v5");
        let media = theme.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        let poster = media.join("poster.jpg");
        std::fs::write(&poster, b"\xff\xd8\xffpackaged-poster").unwrap();
        crate::theme_media::write_poster_record(&dir, Path::new("media/poster.jpg")).unwrap();
        std::fs::write(
            dir.join("video.txt"),
            "path=/nonexistent/other-machine/loop.mp4\npainter=mpvpaper\nnamespace=dp-3\npid=1\n",
        )
        .unwrap();
        assert!(
            !dir.join("wallpaper.txt").exists(),
            "a video theme carries no static record"
        );

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);

        let log = stub_log(&stub);
        let shell_line = format!("msg wallpaper-set  {}", poster.display());
        assert!(
            log.lines().any(|l| l == shell_line),
            "with no video backend the packaged poster must become the static background, log was:\n{log}"
        );
        assert!(
            !log.contains("loop.mp4"),
            "the missing absolute video must not be handed over, log was:\n{log}"
        );
        std::thread::sleep(Duration::from_millis(500));
    }

    // ── Theme packages T6: the video-backend notice ───────────────────
    //
    // WHY these exist: when a theme wants a video but no backend can paint it,
    // the poster fallback stays AND the user must be told what is missing and
    // where to read how to install a backend. The decision + copy are pure
    // (`providers::bg_info::video_backend_notice`); this drives the real apply
    // to prove the notice fires alongside the poster fallback.

    /// A theme carrying BOTH a packaged video and a packaged poster, with no
    /// mpvpaper manifest — the "no backend at all" case T6 adds.
    fn theme_with_packaged_video_and_poster(stub: &ColorStub) -> (TempDir, PathBuf, PathBuf) {
        let theme = stub.custom_theme("wallpaper vibrant", false);
        let dir = theme.path().join("providers").join("noctalia-v5");
        let media = theme.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        let video = media.join(crate::theme_media::VIDEO_FILE_NAME);
        std::fs::write(&video, b"packaged video bytes").unwrap();
        crate::theme_media::write_video_record(&dir, Path::new("media/background.mp4")).unwrap();
        let poster = media.join("poster.jpg");
        std::fs::write(&poster, b"\xff\xd8\xffpackaged-poster").unwrap();
        crate::theme_media::write_poster_record(&dir, Path::new("media/poster.jpg")).unwrap();
        (theme, video, poster)
    }

    /// The T6 acceptance path: a packaged video with no backend paints the
    /// poster statically AND raises the actionable notice naming the wiki
    /// page. Neither half may regress.
    #[test]
    #[serial]
    fn v5_apply_packaged_video_without_backend_notifies_and_paints_the_poster() {
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        // No delegation socket and no enabled plugin: no backend at all.
        let (theme, _video, poster) = theme_with_packaged_video_and_poster(&stub);
        assert!(
            !theme
                .path()
                .join("providers")
                .join("noctalia-v5")
                .join(crate::providers::bg_info::MANIFEST_FILE)
                .exists(),
            "the fixture must carry no mpvpaper manifest"
        );

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {res:?}");

        let log = stub_log(&stub);
        let shell_line = format!("msg wallpaper-set  {}", poster.display());
        assert!(
            log.lines().any(|l| l == shell_line),
            "with no video backend the packaged poster must still be painted, log was:\n{log}"
        );

        let notified = stub.notified();
        assert!(
            notified.contains("Fondo animado no aplicado"),
            "the video-backend notice must fire, notify log was:\n{notified}"
        );
        assert!(
            notified.contains(crate::providers::bg_info::VIDEO_BACKEND_WIKI_URL),
            "the notice must point at the wiki page, notify log was:\n{notified}"
        );
        std::thread::sleep(Duration::from_millis(500));
    }

    /// The counterpart: when a video backend CAN paint, the video wins and
    /// the poster is not also painted statically (that would cover it).
    #[test]
    #[serial]
    fn v5_apply_video_with_backend_keeps_the_video_and_skips_the_poster_static() {
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        enable_delegation(&stub, "0");
        let (theme, poster) = theme_with_packaged_poster(&stub, None);
        let dir = theme.path().join("providers").join("noctalia-v5");
        let video = stub._tmp.path().join("loop.mp4");
        std::fs::write(&video, b"fake video bytes").unwrap();
        std::fs::write(
            dir.join("video.txt"),
            format!(
                "path={}\npainter=skwd-wall-vk\nnamespace=dp-3\npid=1\n",
                video.display()
            ),
        )
        .unwrap();

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);

        let log = stub_log(&stub);
        assert!(
            count_prefix(&stub, "skwd-wall-v2 apply") + count_prefix(&stub, "skwd-helm apply") >= 1,
            "the video must be handed to a video backend, log was:\n{log}"
        );
        let shell_line = format!("msg wallpaper-set  {}", poster.display());
        assert!(
            !log.lines().any(|l| l == shell_line),
            "a painted video must not also paint the poster statically, log was:\n{log}"
        );
        let _ = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(500));
    }

    /// A declared poster whose file is gone falls back to the normal chain.
    #[test]
    #[serial]
    fn v5_apply_declared_poster_with_missing_file_falls_back_to_wallpaper_txt() {
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("wallpaper vibrant", false);
        let dir = theme.path().join("providers").join("noctalia-v5");
        crate::theme_media::write_poster_record(&dir, Path::new("media/gone.jpg")).unwrap();
        std::fs::write(dir.join("wallpaper.txt"), "/tmp/fallback-absolute.png").unwrap();

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);

        let log = stub_log(&stub);
        assert!(
            log.lines().any(|l| l == "msg wallpaper-set  /tmp/fallback-absolute.png"),
            "a declared poster with no file must fall back to wallpaper.txt, log was:\n{log}"
        );
        assert!(
            !log.contains("media/gone.jpg"),
            "the missing poster must not be handed over, log was:\n{log}"
        );
        std::thread::sleep(Duration::from_millis(500));
    }

    /// A leftover `media/poster.*` with NO record must never resurrect a
    /// background the theme no longer declares.
    #[test]
    #[serial]
    fn v5_apply_never_resurrects_a_stray_poster_file() {
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("wallpaper vibrant", false);
        let dir = theme.path().join("providers").join("noctalia-v5");
        let media = theme.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join("poster.jpg"), b"stray leftover").unwrap();
        std::fs::write(dir.join("wallpaper.txt"), "/tmp/real-wall.png").unwrap();

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);

        let log = stub_log(&stub);
        assert!(
            log.lines().any(|l| l == "msg wallpaper-set  /tmp/real-wall.png"),
            "the normal chain must still be used, log was:\n{log}"
        );
        assert!(
            !log.contains("media/poster.jpg"),
            "a stray media file with no record must never be chosen, log was:\n{log}"
        );
        std::thread::sleep(Duration::from_millis(500));
    }

    /// A record that tries to escape the theme (`..`) must be rejected and
    /// the apply must fall back — never fail, never resolve the escape.
    #[test]
    #[serial]
    fn v5_apply_rejects_a_traversal_record_and_falls_back() {
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("wallpaper vibrant", false);
        let dir = theme.path().join("providers").join("noctalia-v5");
        crate::theme_media::write_poster_record(&dir, Path::new("../../../etc/passwd")).unwrap();
        std::fs::write(dir.join("wallpaper.txt"), "/tmp/safe-wall.png").unwrap();

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(
            res.is_ok(),
            "a bad poster record must never fail the apply: {:?}",
            res
        );

        let log = stub_log(&stub);
        assert!(
            log.lines().any(|l| l == "msg wallpaper-set  /tmp/safe-wall.png"),
            "a rejected record must fall back to wallpaper.txt, log was:\n{log}"
        );
        assert!(
            !log.contains("passwd"),
            "the traversal target must never be handed over, log was:\n{log}"
        );
        std::thread::sleep(Duration::from_millis(500));
    }

    // ── Theme packages T3: apply resolves the video theme-first ───────
    //
    // WHY these exist: a theme may carry its video (`media/background.mp4`)
    // or, when the video was too large to copy, a recorded url. Apply must
    // resolve the packaged file first, keep the old absolute `video.txt`
    // path working, and hand an over-bound video's url to the SAME mpvpaper
    // backend that plays any saved manifest.

    /// A theme carrying a packaged video (`media/background.mp4` + the
    /// theme-relative `video-media.txt`), with an optional absolute `video.txt`
    /// pointing at a real file elsewhere.
    fn theme_with_packaged_video(stub: &ColorStub, legacy: Option<&Path>) -> (TempDir, PathBuf) {
        let theme = stub.custom_theme("wallpaper vibrant", false);
        let dir = theme.path().join("providers").join("noctalia-v5");
        let media = theme.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        let video = media.join(crate::theme_media::VIDEO_FILE_NAME);
        std::fs::write(&video, b"packaged video bytes").unwrap();
        crate::theme_media::write_video_record(&dir, Path::new("media/background.mp4")).unwrap();
        if let Some(legacy) = legacy {
            std::fs::write(
                dir.join("video.txt"),
                format!(
                    "path={}\npainter=skwd-wall-vk\nnamespace=dp-3\npid=1\n",
                    legacy.display()
                ),
            )
            .unwrap();
        }
        (theme, video)
    }

    /// The packaged video wins over the (still readable) absolute path, and is
    /// handed to the engine as the theme's OWN file.
    #[test]
    #[serial]
    fn v5_apply_prefers_the_packaged_video_over_the_absolute_path() {
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        enable_delegation(&stub, "0");
        let legacy = stub._tmp.path().join("legacy-loop.mp4");
        std::fs::write(&legacy, b"legacy video").unwrap();
        let (theme, packaged) = theme_with_packaged_video(&stub, Some(&legacy));

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {res:?}");

        let log = stub_log(&stub);
        let line = format!("skwd-helm apply {}", packaged.display());
        let alt = format!("skwd-wall-v2 apply {}", packaged.display());
        assert!(
            log.lines().any(|l| l == line || l == alt),
            "the packaged video must be handed to the engine, log was:\n{log}"
        );
        assert!(
            !log.contains(&legacy.display().to_string()),
            "the absolute legacy path must not win over the packaged video, log was:\n{log}"
        );
        let _ = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(500));
    }

    /// An over-bound video was never copied: apply hands its recorded url to
    /// the mpvpaper backend by filling in the manifest the plugin plays.
    #[test]
    #[serial]
    fn v5_apply_hands_a_recorded_url_to_the_mpvpaper_backend() {
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        // Keep only the stub bin dir on PATH: no notify-send/pkill/curl runs.
        std::env::set_var("PATH", stub._tmp.path().join("bin"));
        let theme = stub.custom_theme("wallpaper vibrant", false);
        let dir = theme.path().join("providers").join("noctalia-v5");
        crate::theme_media::write_video_url_record(
            &dir,
            "https://example.com/clip.mp4",
            Some("clip.mp4"),
            None,
        )
        .unwrap();
        assert!(!dir.join("video.txt").exists(), "url-only theme has no absolute record");
        assert!(
            !dir.join(crate::providers::bg_info::MANIFEST_FILE).exists(),
            "url-only theme carries no manifest yet"
        );

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {res:?}");

        let manifest = std::fs::read_to_string(dir.join(crate::providers::bg_info::MANIFEST_FILE))
            .expect("the url must be handed to the mpvpaper backend as a manifest");
        assert!(
            manifest.contains("https://example.com/clip.mp4"),
            "the manifest must carry the recorded url, was:\n{manifest}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }

    /// An old theme with ONLY the absolute `video.txt` path resolves exactly as
    /// before the packaged-video support existed.
    #[test]
    #[serial]
    fn v5_apply_old_theme_with_only_the_absolute_video_path_is_unchanged() {
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        enable_delegation(&stub, "0");
        let theme = stub.custom_theme("wallpaper vibrant", false);
        let dir = theme.path().join("providers").join("noctalia-v5");
        let video = stub._tmp.path().join("loop.mp4");
        std::fs::write(&video, b"legacy video").unwrap();
        std::fs::write(
            dir.join("video.txt"),
            format!(
                "path={}\npainter=skwd-wall-vk\nnamespace=dp-3\npid=1\n",
                video.display()
            ),
        )
        .unwrap();

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {res:?}");

        let log = stub_log(&stub);
        let line = format!("skwd-helm apply {}", video.display());
        let alt = format!("skwd-wall-v2 apply {}", video.display());
        assert!(
            log.lines().any(|l| l == line || l == alt),
            "the absolute path must still be handed over, log was:\n{log}"
        );
        let _ = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(500));
    }

    /// Phase 2 wiring: applying a video theme whose saved settings.toml
    /// re-arms Noctalia's rotation must turn the LIVE flag off, keeping
    /// every other restored byte intact.
    #[test]
    #[serial]
    fn v5_apply_video_neutralizes_the_restored_rotation() {
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_JSON), true);
        enable_delegation(&stub, "0");
        let theme = animated_theme(&stub);
        let dir = theme.path().join("providers").join("noctalia-v5");
        let saved = "[wallpaper]\n\
                     directory = \"/home/u/Pictures/Wallpapers/Cars\"\n\
                     \n\
                     [wallpaper.automation]\n\
                     enabled = true\n\
                     interval = 600\n";
        std::fs::write(dir.join("settings.toml"), saved).unwrap();
        // The live Noctalia state dir exists in production; create it so the
        // restore (and the Phase 2 edit of the LIVE file) has a target.
        let home = std::env::var("HOME").unwrap();
        let live_dir = PathBuf::from(&home).join(".local/state/noctalia");
        std::fs::create_dir_all(&live_dir).unwrap();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let live = std::fs::read_to_string(live_dir.join("settings.toml"))
            .expect("the live settings.toml must exist after apply");
        assert!(
            live.contains("[wallpaper.automation]\nenabled = false\n"),
            "apply must neutralize the rotation it restored, live was:\n{live}"
        );
        assert!(
            live.contains("interval = 600") && live.contains("Wallpapers/Cars"),
            "only the automation flag may change, live was:\n{live}"
        );
        let _ = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(500));
    }

    #[test]
    #[serial]
    fn v5_apply_animated_yields_before_the_video_hand_off() {
        // (a) ORDERING, not mere co-presence: with the yield hoisted before
        // the whole wallpaper phase, the engine config must already read
        // `off` when the ANIMATED hand-off (`skwd-helm apply <video>`)
        // runs. Before W4 the yield sat inside the static branch — skipped
        // once the video applied — so the delegation probe reported
        // `delegation saw no yield` and the flash stayed.
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_TRAP_JSON), true);
        enable_delegation(&stub, "0");
        let theme = animated_theme(&stub);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert!(
            count_prefix(&stub, "skwd-helm apply") >= 1,
            "the video must be handed to the engine"
        );
        assert!(
            count_prefix(&stub, "delegation saw policy off") >= 1,
            "the yield must precede the video hand-off (ordering)"
        );
        assert_eq!(
            count_prefix(&stub, "delegation saw no yield"),
            0,
            "no hand-off may run before the yield"
        );
        // W1: the theme owns the palette, so after the worker verifies and
        // releases, the mute must still hold — config `off`, marker pending.
        let sets = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        assert_eq!(sets, 2, "one sync set plus exactly one re-assert");
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            std::fs::read_to_string(&stub.skwd_cfg)
                .unwrap()
                .contains("\"policy\": \"off\""),
            "the mute must outlive the animated apply while the theme rules"
        );
        assert!(
            background::color_authority_held(),
            "the held mute keeps the crash marker"
        );
    }

    #[test]
    #[serial]
    fn v5_apply_static_yields_before_the_static_hand_off() {
        // (b) A static-only apply: W4 moves the yield earlier, it must not
        // alter the static path's guarantees — the yield still precedes the
        // static hand-off and the re-assert still fires. W1: because this
        // theme owns the palette, the mute then holds (config stays `off`,
        // marker stays pending) instead of restoring.
        //
        // Engine order (2026-09-26): a STILL image now goes to the fast engine
        // first (`skwd-wall-v2` ~3ms vs `skwd-helm` ~616ms), the other staying
        // as fallback. The guarantee under test is untouched: whichever engine
        // does the hand-off, the yield must already have happened.
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_JSON), true);
        enable_delegation(&stub, "0");
        let theme = stub.custom_theme("custom JokerTheme", true);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert!(
            count_prefix(&stub, "skwd-wall-v2 apply") >= 1,
            "a still image must be handed to the fast engine"
        );
        assert_eq!(
            count_prefix(&stub, "delegation saw no yield"),
            0,
            "the yield must precede the static hand-off too"
        );
        assert!(
            count_prefix(&stub, "delegation saw policy off") >= 1,
            "the static hand-off must see the flip"
        );
        // W1: the theme owns the palette, so the mute must outlive the
        // static apply too — config stays `off`, marker stays pending.
        let sets = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        assert_eq!(sets, 2, "re-assert behaviour unchanged");
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            std::fs::read_to_string(&stub.skwd_cfg)
                .unwrap()
                .contains("\"policy\": \"off\""),
            "the mute must outlive the static apply while the theme rules"
        );
        assert!(
            background::color_authority_held(),
            "the held mute keeps the crash marker"
        );
    }

    #[test]
    #[serial]
    fn v5_apply_both_branches_single_yield_single_hold() {
        // (c) One apply in which BOTH wallpaper branches run (the animated
        // hand-off fails, so the static restore follows): exactly ONE
        // yield, ONE marker, ONE hold. The yield covers the whole phase,
        // never per-branch. Mid-apply the authority is held off (the video
        // hand-off already saw the flip), the marker exists; W1 keeps that
        // state after the worker verifies, because this theme owns the
        // palette.
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_TRAP_JSON), false);
        enable_delegation(&stub, "1"); // both delegations fail -> static runs too
        let theme = animated_theme(&stub);
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::INFO)
            .finish();
        let _tracing = tracing::subscriber::set_default(subscriber);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert_eq!(
            count_prefix(&stub, "delegation saw no yield"),
            0,
            "the video hand-off attempt must already see the flip"
        );
        // Mid-apply (production timing, worker settle 2 s): held off, marker
        // present — exactly one yield in flight.
        let held = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert!(
            held.contains("\"policy\": \"off\""),
            "the authority must be held off through the apply, got: {}",
            held
        );
        assert!(
            background::color_authority_held(),
            "one active yield -> one marker"
        );
        // W1: the theme owns the palette, so the single release keeps the
        // mute: one yield, one marker, and the file never moves again.
        let sets = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        assert_eq!(sets, 2, "one sync set plus exactly one re-assert");
        std::thread::sleep(Duration::from_millis(1500));
        drop(_tracing);
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert_eq!(
            text.matches("engine colour authority yielded for the apply").count(),
            1,
            "exactly one yield per apply, got: {}",
            text
        );
        assert_eq!(
            std::fs::read_to_string(&stub.skwd_cfg).unwrap(),
            held,
            "the held mute must not rewrite one byte after the flip"
        );
        assert!(
            background::color_authority_held(),
            "the held mute keeps its single marker"
        );
    }

    #[test]
    #[serial]
    fn v5_apply_animated_early_error_still_restores() {
        // (d) Early return AFTER an animated-path yield: the video hand-off
        // succeeded, then the palette target cannot be created and apply
        // fails — the guard's Drop must restore and log.
        let stub = ColorStub::new("custom skwd-wall", 99, Some(OWNER_TRAP_JSON), true);
        enable_delegation(&stub, "0");
        let theme = animated_theme(&stub);
        let broken =
            std::env::temp_dir().join(format!("hve-noct-broken-w4-{}", std::process::id()));
        std::fs::write(&broken, b"file, not a dir").unwrap();
        std::env::set_var("HVE_NOCTALIA_CONFIG", &broken);
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _tracing = tracing::subscriber::set_default(subscriber);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_err(), "the broken palette target must fail apply");
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, OWNER_TRAP_JSON,
            "an early error after an animated yield must still restore: {}",
            cfg
        );
        assert!(
            !background::color_authority_held(),
            "restore clears the marker"
        );
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("yield guard"),
            "the guard's scope-exit restore must be logged, got: {}",
            text
        );
        let _ = std::fs::remove_file(&broken);
    }

    #[test]
    #[serial]
    fn v5_apply_animated_no_palette_restores_synchronously() {
        // (d) No-palette branch: an animated hand-off happened (yield
        // fired) but there is no custom palette to verify — no worker is
        // spawned, so the guard must restore synchronously at the end of
        // apply.
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        enable_delegation(&stub, "0");
        let theme = animated_theme(&stub);
        // Downgrade to a non-custom source with no palette.
        let dir = theme.path().join("providers").join("noctalia-v5");
        std::fs::write(dir.join("source.txt"), "wallpaper vibrant").unwrap();
        std::fs::remove_file(dir.join("palette.json")).unwrap();
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _tracing = tracing::subscriber::set_default(subscriber);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert!(
            count_prefix(&stub, "skwd-helm apply") >= 1,
            "the animated hand-off ran (yield fired)"
        );
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, OWNER_JSON,
            "the no-palette branch must restore synchronously: {}",
            cfg
        );
        assert!(!background::color_authority_held());
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("yield guard"),
            "the guard's scope-exit restore must be logged, got: {}",
            text
        );
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 0, "non-custom palette must issue no set");
    }

    #[test]
    #[serial]
    fn v5_apply_animated_without_ownership_writes_nothing() {
        // (e) Animated theme, NO engine config: nothing to yield, nothing
        // written anywhere, marker absent — and the video hand-off still
        // runs exactly as today.
        let stub = ColorStub::new("custom skwd-wall", 99, None, true);
        enable_delegation(&stub, "0");
        let theme = animated_theme(&stub);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert!(
            !stub.skwd_cfg.exists(),
            "no config file must mean no yield and no write"
        );
        assert!(
            !background::color_authority_held(),
            "no yield must leave no marker"
        );
        assert!(
            count_prefix(&stub, "skwd-helm apply") >= 1,
            "the animated hand-off runs unchanged"
        );
        assert_eq!(
            count_prefix(&stub, "delegation saw policy off"),
            0,
            "no config -> the flip never happened"
        );
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 1, "only the synchronous set, zero extra");
        assert_eq!(stub.templates(), 1, "only the synchronous templates-apply");
    }

    #[test]
    #[serial]
    fn v5_apply_animated_fixed_policy_writes_nothing() {
        // (e) Animated theme, `fixed` policy: no ownership, no yield — the
        // engine config bytes must stay exactly as they were, marker
        // absent, hand-off behaviour untouched.
        let fixed = r#"{"theme": {"policy": "fixed"}, "noctalia": {"themeMode": "manual"}}"#;
        let stub = ColorStub::new("custom skwd-wall", 99, Some(fixed), true);
        enable_delegation(&stub, "0");
        let theme = animated_theme(&stub);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, fixed,
            "a fixed policy must leave the engine config byte-identical: {}",
            cfg
        );
        assert!(!background::color_authority_held());
        assert_eq!(
            count_prefix(&stub, "delegation saw policy off"),
            0,
            "nothing was ever flipped"
        );
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 1, "only the synchronous set, zero extra");
        assert_eq!(stub.templates(), 1, "only the synchronous templates-apply");
    }

    #[test]
    #[serial]
    fn v5_apply_no_wallpaper_assets_never_yields_even_when_owner() {
        // (e) A theme with NO wallpaper to hand over (no video.txt, no
        // wallpaper.txt) must not flip anything, even though the engine
        // owns colours: nothing is handed over, so nothing to yield — the
        // engine config stays byte-identical and the re-assert behaves as
        // today.
        let stub = ColorStub::new("custom skwd-wall", 99, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, OWNER_JSON,
            "no wallpaper assets -> engine config untouched: {}",
            cfg
        );
        assert!(!background::color_authority_held());
        // The re-assert still runs, exactly as today: the owner never
        // yields, so the worker makes its bounded attempts (1 sync set plus
        // at most 3 re-asserts), never a war.
        let sets = stub.poll("color-scheme-set", 4, Duration::from_secs(5));
        assert_eq!(sets, 4, "owner re-assert keeps its bounded shape");
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(stub.sets(), 4, "gave up at the cap: no war");
    }

    /// The re-assert worker is DETACHED and reads PROCESS-GLOBAL state:
    /// `PATH` to resolve this stub's `noctalia`, and `SKWD_WALL_V2_CONFIG` /
    /// `HOME` to reach the engine config and the colour-authority descriptor.
    /// With the production timing profile the worker lives ~4 s — well past
    /// the apply — so before the handshake it outlived its test and leaked
    /// its later `color-scheme-set` calls into the NEXT serial test's stub
    /// log (and its release could rewrite that test's engine config). This
    /// pins the `ColorStub::drop` handshake: right after the worker's first
    /// re-assert it is mid-delay, so a drop that did NOT wait would leave
    /// the counter non-zero deterministically.
    #[test]
    #[serial]
    fn a_detached_reassert_worker_never_outlives_its_stub_sandbox() {
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_JSON), false);
        let theme = stub.custom_theme("custom JokerTheme", true);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let sets = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        assert_eq!(sets, 2, "one sync set plus the worker's first re-assert");
        assert!(
            !reassert_workers_idle(),
            "precondition: the production-timing worker is still mid-delay"
        );
        drop(stub); // must block until the worker finishes
        assert!(
            reassert_workers_idle(),
            "dropping the stub must wait for its detached worker"
        );
    }

    // ── Save-side static background: record what the painter shows ──
    //
    // WHY these exist: the keeper changed colors + background, then
    // overwrote a theme. Save recorded `wallpaper-get` (Noctalia's stale
    // car3.jpg) while the skwd suite actually painted joker1.png, so the
    // gallery previewed the wrong image and a later apply would restore
    // it. The static arm must prefer the painter's own truth when the
    // compositor proves the skwd suite paints a static scene.
    //
    // Hermetic harness: stub `noctalia` / `hyprctl` / `skwd-helm` /
    // `skwd-wall-v2` are shadowed onto PATH. A real spawn would query the
    // keeper's live desktop (and Noctalia IPC can mutate it) — these tests
    // must never run the real binaries.

    /// Stub fleet for the v5 save path: answers `color-scheme-get`,
    /// `wallpaper-get`, `hyprctl -j layers` / `monitors` and the daemon's
    /// `outputs --json` from files under a private state dir.
    struct SaveStub {
        _env: crate::test_utils::TempEnv,
        _tmp: TempDir,
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl SaveStub {
        /// `layers` is the `hyprctl -j layers` fixture; `monitors` `None`
        /// means the monitors query fails (the parser then stays
        /// permissive, logging the gap). `daemon_mode` `"ok"` serves
        /// `daemon_json`, `"garbage"` serves unparseable output, anything
        /// else makes both daemon binaries fail.
        fn new(
            layers: &str,
            monitors: Option<&str>,
            daemon_mode: &str,
            daemon_json: &str,
            wallpaper_get: &str,
        ) -> Self {
            let env = crate::test_utils::TempEnv::new();
            let tmp = TempDir::new().unwrap();
            let keys = [
                "PATH",
                "HVE_NOCTALIA_CONFIG",
                "NOCTALIA_STUB_STATE",
                "SKWD_STUB_MODE",
                "XDG_RUNTIME_DIR",
                "SKWD_WALL_V2_SOCK",
                "SKWD_WALL_V2_CONFIG",
            ];
            let saved: Vec<(&'static str, Option<String>)> = keys
                .iter()
                .map(|k| (*k, std::env::var(k).ok()))
                .collect();
            let state_dir = tmp.path().join("state");
            let bin_dir = tmp.path().join("bin");
            std::fs::create_dir_all(&state_dir).unwrap();
            std::fs::create_dir_all(&bin_dir).unwrap();
            std::fs::write(state_dir.join("layers.json"), layers).unwrap();
            if let Some(m) = monitors {
                std::fs::write(state_dir.join("monitors.json"), m).unwrap();
            }
            std::fs::write(state_dir.join("daemon.json"), daemon_json).unwrap();
            std::fs::write(state_dir.join("wallpaper-get"), wallpaper_get).unwrap();
            // The scheme `color-scheme-get` answers with; `with_scheme`
            // overrides it for tests that need another palette source.
            std::fs::write(state_dir.join("scheme"), "custom JokerTheme\n").unwrap();
            std::fs::write(state_dir.join("log"), b"").unwrap();
            let noctalia_stub = r#"#!/bin/sh
STATE="${NOCTALIA_STUB_STATE:?missing stub state}"
printf '%s\n' "$*" >> "$STATE/log"
if [ "$1" = "msg" ] && [ "$2" = "color-scheme-get" ]; then
  cat "$STATE/scheme"
  exit 0
fi
if [ "$1" = "msg" ] && [ "$2" = "wallpaper-get" ]; then
  cat "$STATE/wallpaper-get"
  exit 0
fi
exit 0
"#;
            let hyprctl_stub = r#"#!/bin/sh
STATE="${NOCTALIA_STUB_STATE:?missing stub state}"
if [ "$1" = "-j" ] && [ "$2" = "layers" ]; then
  cat "$STATE/layers.json"
  exit 0
fi
if [ "$1" = "-j" ] && [ "$2" = "monitors" ]; then
  if [ -f "$STATE/monitors.json" ]; then
    cat "$STATE/monitors.json"
    exit 0
  fi
  exit 1
fi
exit 1
"#;
            let helm_stub = r#"#!/bin/sh
STATE="${NOCTALIA_STUB_STATE:?missing stub state}"
MODE="${SKWD_STUB_MODE:-ok}"
if [ "$1" = "outputs" ] && [ "$2" = "--json" ]; then
  case "$MODE" in
    ok) cat "$STATE/daemon.json"; exit 0;;
    garbage) printf 'not json at all'; exit 0;;
    *) exit 1;;
  esac
fi
exit 1
"#;
            for (name, content) in [
                ("noctalia", noctalia_stub),
                ("hyprctl", hyprctl_stub),
                ("skwd-helm", helm_stub),
                ("skwd-wall-v2", "#!/bin/sh\nexit 1\n"),
            ] {
                let p = bin_dir.join(name);
                std::fs::write(&p, content).unwrap();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))
                        .unwrap();
                }
            }
            let orig_path = std::env::var("PATH").unwrap_or_default();
            std::env::set_var(
                "PATH",
                format!("{}:{}", bin_dir.display(), orig_path),
            );
            std::env::set_var("NOCTALIA_STUB_STATE", &state_dir);
            std::env::set_var("SKWD_STUB_MODE", daemon_mode);
            let noct_home = tmp.path().join("noct-home");
            std::fs::create_dir_all(&noct_home).unwrap();
            std::env::set_var("HVE_NOCTALIA_CONFIG", &noct_home);
            // No daemon socket in tests: delegation stays a silent no-op.
            std::env::set_var("XDG_RUNTIME_DIR", tmp.path());
            std::env::remove_var("SKWD_WALL_V2_SOCK");
            std::env::set_var(
                "SKWD_WALL_V2_CONFIG",
                tmp.path().join("no-skwd-config.json"),
            );
            Self {
                _env: env,
                _tmp: tmp,
                saved,
            }
        }

        fn provider_dir(&self, theme: &TempDir) -> PathBuf {
            theme.path().join("providers").join("noctalia-v5")
        }

        /// Answer `color-scheme-get` with another live scheme, so a save can
        /// be driven for any palette source (the default is `custom
        /// JokerTheme`).
        fn with_scheme(self, scheme: &str) -> Self {
            std::fs::write(self._tmp.path().join("state").join("scheme"), scheme).unwrap();
            self
        }

        fn wallpaper_txt(&self, theme: &TempDir) -> Option<String> {
            std::fs::read_to_string(self.provider_dir(theme).join("wallpaper.txt"))
                .ok()
                .map(|s| s.trim().to_string())
        }

        /// The stub `noctalia` argv log, so a save+apply flow can assert which
        /// wallpaper was actually handed to the shell.
        fn logged(&self) -> String {
            std::fs::read_to_string(self._tmp.path().join("state").join("log")).unwrap_or_default()
        }
    }

    impl Drop for SaveStub {
        fn drop(&mut self) {
            for (k, v) in &self.saved {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    /// Two-output layers fixture with the given topmost namespaces,
    /// fullscreen opaque on both outputs.
    fn save_layers_doc(dp_ns: &str, hdmi_ns: &str) -> String {
        format!(
            r#"{{"DP-3":{{"levels":{{"0":[{{"address":"0x1","x":0,"y":0,"w":2560,"h":1440,"alpha":1,"namespace":"{dp_ns}","pid":439101}}],"1":[]}}}},"HDMI-A-1":{{"levels":{{"0":[{{"address":"0x2","x":0,"y":0,"w":1440,"h":900,"alpha":1,"namespace":"{hdmi_ns}","pid":439102}}],"1":[]}}}}}}"#,
        )
    }

    /// Two-output daemon fixture of one kind word with the given currents.
    fn save_daemon_doc(kind_word: &str, dp_current: &str, hdmi_current: &str) -> String {
        format!(
            r#"{{"outputs": [{{"name": "DP-3", "current": "{dp_current}", "path": "{dp_current}", "type": "{kind_word}", "connected": true}},{{"name": "HDMI-A-1", "current": "{hdmi_current}", "path": "{hdmi_current}", "type": "{kind_word}", "connected": true}}]}}"#,
        )
    }

    /// A save is a CAPTURE of the live truth: a theme whose declared palette
    /// cannot be resolved must not keep the `palette.json` an earlier save
    /// left behind. The stub answers `custom JokerTheme` while its noctalia
    /// home carries no such palette, so nothing resolves — the pre-existing
    /// file is the leftover that must go.
    #[test]
    #[serial]
    fn v5_save_drops_a_stale_palette_the_declared_source_cannot_resolve() {
        let stub = SaveStub::new(
            &save_layers_doc("skwd-paper", "skwd-paper"),
            None,
            "ok",
            &save_daemon_doc("static", "/pictures/live/joker1.png", "/pictures/live/joker1.png"),
            "/pictures/stale/car3.jpg",
        );
        let theme = TempDir::new().unwrap();
        let dir = stub.provider_dir(&theme);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("palette.json"), r#"{"palette":"joker"}"#).unwrap();

        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");

        assert!(
            !dir.join("palette.json").exists(),
            "a theme whose declared palette does not resolve must not keep a foreign palette"
        );
    }

    /// The other half of the invariant: when the declared palette DOES
    /// resolve, the captured bytes are that palette's, byte for byte.
    #[test]
    #[serial]
    fn v5_save_captures_the_resolved_palette_byte_for_byte() {
        let stub = SaveStub::new(
            &save_layers_doc("skwd-paper", "skwd-paper"),
            None,
            "ok",
            &save_daemon_doc("static", "/pictures/live/joker1.png", "/pictures/live/joker1.png"),
            "/pictures/stale/car3.jpg",
        );
        let theme = TempDir::new().unwrap();
        let dir = stub.provider_dir(&theme);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("palette.json"), r#"{"palette":"joker"}"#).unwrap();
        // The live custom palette the stub's `color-scheme-get` names.
        let live = PathBuf::from(std::env::var("HVE_NOCTALIA_CONFIG").unwrap())
            .join("palettes")
            .join("JokerTheme.json");
        std::fs::create_dir_all(live.parent().unwrap()).unwrap();
        std::fs::write(&live, br##"{"dark":{"mPrimary":"#74c7ec"}}"##).unwrap();

        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");

        assert_eq!(
            std::fs::read(dir.join("palette.json")).unwrap(),
            std::fs::read(&live).unwrap(),
            "the captured palette must be the declared palette's bytes"
        );
    }

    /// The exact live case, end to end: the theme declares
    /// `community Catppuccin Mocha Sapphire`, Noctalia holds that palette
    /// under its ENCODED file name, and the save must capture those bytes
    /// instead of the JokerTheme palette an earlier save left in the theme.
    #[test]
    #[serial]
    fn v5_save_captures_a_community_palette_whose_name_has_spaces() {
        let stub = SaveStub::new(
            &save_layers_doc("skwd-paper", "skwd-paper"),
            None,
            "ok",
            &save_daemon_doc("static", "/pictures/live/joker1.png", "/pictures/live/joker1.png"),
            "/pictures/stale/car3.jpg",
        )
        .with_scheme("community Catppuccin Mocha Sapphire\n");
        let theme = TempDir::new().unwrap();
        let dir = stub.provider_dir(&theme);
        std::fs::create_dir_all(&dir).unwrap();
        // The leftover a previous save wrote: the JokerTheme palette.
        std::fs::write(
            dir.join("palette.json"),
            br##"{"dark":{"mPrimary":"#2ec436"}}"##,
        )
        .unwrap();

        // Noctalia's own community store, under the file name it writes.
        let community = PathBuf::from(std::env::var("HOME").unwrap())
            .join(".local")
            .join("state")
            .join("noctalia")
            .join("community-palettes");
        std::fs::create_dir_all(&community).unwrap();
        std::fs::write(
            community.join("Catppuccin%20Mocha%20Sapphire.json"),
            br##"{"dark":{"mPrimary":"#74c7ec","mSurface":"#1e1e2e"}}"##,
        )
        .unwrap();

        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");

        let captured = String::from_utf8(std::fs::read(dir.join("palette.json")).unwrap()).unwrap();
        assert!(
            captured.contains("#74c7ec"),
            "the captured palette must be the declared community palette: {captured}"
        );
        assert!(
            !captured.contains("#2ec436"),
            "the leftover JokerTheme palette must not survive the capture: {captured}"
        );
    }

    /// A real image file with distinct bytes, so "which path did the save
    /// record?" stays observable through the packaged poster: the absolute
    /// background records no longer survive a save (U2), and a fake path has
    /// no poster to package.
    fn real_wallpaper(dir: &Path, name: &str, bytes: &[u8]) -> String {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    #[serial]
    fn v5_save_skwd_static_records_daemon_current_not_wallpaper_get() {
        // The keeper's case: Noctalia reports the stale car3.jpg while the
        // suite paints joker1.png on both outputs. Save must record what
        // the painter actually has — read from the packaged poster, because
        // the absolute record does not survive the save.
        let fixtures = TempDir::new().unwrap();
        let stale = real_wallpaper(fixtures.path(), "car3.jpg", b"stale-wallpaper-get");
        let live_path = real_wallpaper(fixtures.path(), "joker1.png", b"daemon-current");
        let stub = SaveStub::new(
            &save_layers_doc("skwd-paper", "skwd-paper"),
            None,
            "ok",
            &save_daemon_doc("static", &live_path, &live_path),
            &stale,
        );
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");
        assert_eq!(
            std::fs::read(theme.path().join("media").join("poster.png")).unwrap(),
            b"daemon-current",
            "skwd paints a static scene: the save must record the daemon current, not the stale wallpaper-get"
        );
        assert!(
            !theme.path().join("media").join("poster.jpg").exists(),
            "the stale wallpaper-get value must never be packaged"
        );
        assert!(
            stub.wallpaper_txt(&theme).is_none(),
            "the absolute wallpaper record must not survive a save"
        );
    }

    #[test]
    #[serial]
    fn v5_save_partial_scene_records_proven_output_current() {
        // The keeper's live partial scene: DP-3 has NO attributable
        // background layer (only a small Noctalia widget at level 1, which
        // the monitors reference excludes as furniture) while HDMI-A-1 is
        // skwd-painted fullscreen static. The daemon reports static joker1
        // for both; wallpaper-get is the stale car3. Absence is not
        // disagreement: save must record the proven output's current.
        let fixtures = TempDir::new().unwrap();
        let live_path = real_wallpaper(fixtures.path(), "joker1.png", b"proven-output-current");
        let stale = real_wallpaper(fixtures.path(), "car3.jpg", b"stale-wallpaper-get");
        let layers = r#"{"DP-3":{"levels":{"0":[],"1":[{"address":"0x1","x":0,"y":0,"w":272,"h":144,"alpha":1,"namespace":"noctalia-desktop-widget-weather-0000000000000001","pid":250289}]}},"HDMI-A-1":{"levels":{"0":[{"address":"0x2","x":0,"y":0,"w":1440,"h":900,"alpha":1,"namespace":"skwd-paper","pid":439102}],"1":[]}}}"#;
        let monitors = r#"[{"name":"DP-3","width":2560,"height":1440,"x":0,"y":0,"scale":1.0},{"name":"HDMI-A-1","width":1440,"height":900,"x":0,"y":0,"scale":1.0}]"#;
        let daemon = save_daemon_doc("static", &live_path, &live_path);
        let stub = SaveStub::new(layers, Some(monitors), "ok", &daemon, &stale);
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");
        assert_eq!(
            std::fs::read(theme.path().join("media").join("poster.png")).unwrap(),
            b"proven-output-current",
            "partial scene (one output absent, one proven skwd static): the save must record the daemon current, not the stale wallpaper-get"
        );
        assert!(
            stub.wallpaper_txt(&theme).is_none(),
            "the absolute wallpaper record must not survive a save"
        );
    }

    #[test]
    #[serial]
    fn v5_save_skwd_static_vs_video_conflict_keeps_wallpaper_get() {
        // A genuine conflict: HDMI-A-1 proven skwd-painted static but DP-3
        // proven skwd-painted video. The static record must not claim the
        // screen: with the video identity unresolvable (daemon names a file
        // that does not exist) the save vetoes honestly and keeps the
        // wallpaper-get value, writing no video record.
        let fixtures = TempDir::new().unwrap();
        let stale = real_wallpaper(fixtures.path(), "car3.jpg", b"wallpaper-get-value");
        let layers = save_layers_doc("skwd-wall-vk", "skwd-paper");
        let daemon = r#"{"outputs": [{"name": "DP-3", "current": "/nonexistent/slugcat.mp4", "path": "/nonexistent/slugcat.mp4", "type": "video", "connected": true},{"name": "HDMI-A-1", "current": "/pictures/live/joker1.png", "path": "/pictures/live/joker1.png", "type": "static", "connected": true}]}"#;
        let stub = SaveStub::new(&layers, None, "ok", daemon, &stale);
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");
        assert_eq!(
            std::fs::read(theme.path().join("media").join("poster.jpg")).unwrap(),
            b"wallpaper-get-value",
            "static-vs-video conflict: the save must keep the wallpaper-get value"
        );
        assert!(
            !stub.provider_dir(&theme).join("video.txt").exists(),
            "unresolvable video: no painter video record may appear"
        );
        assert!(
            stub.wallpaper_txt(&theme).is_none(),
            "the absolute wallpaper record must not survive a save"
        );
    }

    /// The static save path must reuse the authority inputs the kind
    /// decision already captured (one bounded snapshot per save), never run
    /// the full compositor + daemon + monitors capture a second time.
    /// Comment lines are stripped first so a commented-out call cannot
    /// satisfy this. Needles are built with concat() so this test's own
    /// source — it lives in the file it inspects — can never satisfy them.
    #[test]
    fn v5_save_shares_one_authority_snapshot_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            code.contains("query_authority_snapshot"),
            "noctalia.rs save must capture the authority inputs once and share them"
        );
        for needle in [
            ["query_active_wall", "paper_kind("].concat(),
            ["query_active_back", "ground("].concat(),
            ["query_active_static", "_path("].concat(),
        ] {
            assert!(
                !code.contains(&needle),
                "save must reuse the snapshot instead of re-querying via {}",
                needle
            );
        }
    }

    #[test]
    #[serial]
    fn v5_save_non_skwd_painter_keeps_wallpaper_get() {
        // An unknown painter owns the layer: the suite has nothing to say,
        // so save keeps today's behaviour byte-for-byte — read from the
        // packaged poster, since the absolute record does not survive.
        let fixtures = TempDir::new().unwrap();
        let stale = real_wallpaper(fixtures.path(), "car3.jpg", b"wallpaper-get-value");
        let stub = SaveStub::new(
            &save_layers_doc("mystery-layer", "mystery-layer"),
            None,
            "ok",
            &save_daemon_doc("static", "/pictures/live/joker1.png", "/pictures/live/joker1.png"),
            &stale,
        );
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");
        assert_eq!(
            std::fs::read(theme.path().join("media").join("poster.jpg")).unwrap(),
            b"wallpaper-get-value",
            "non-skwd painter: the save must keep the wallpaper-get value"
        );
        assert!(
            !stub.provider_dir(&theme).join("video.txt").exists(),
            "no video on top: no painter video record may appear"
        );
        assert!(
            stub.wallpaper_txt(&theme).is_none(),
            "the absolute wallpaper record must not survive a save"
        );
    }

    #[test]
    #[serial]
    fn v5_save_skwd_daemon_unusable_falls_back_with_warning() {
        // The suite paints, but its daemon answers garbage: save must fall
        // back to wallpaper-get (never an empty or guessed path) and warn
        // naming the output and the cause.
        let fixtures = TempDir::new().unwrap();
        let stale = real_wallpaper(fixtures.path(), "car3.jpg", b"wallpaper-get-value");
        let _stub = SaveStub::new(
            &save_layers_doc("skwd-paper", "skwd-paper"),
            None,
            "garbage",
            "",
            &stale,
        );
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");
        assert_eq!(
            std::fs::read(theme.path().join("media").join("poster.jpg")).unwrap(),
            b"wallpaper-get-value",
            "unusable daemon: the save must fall back to the wallpaper-get value"
        );
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("DP-3"),
            "the fallback warning must name the output, got: {}",
            text
        );
        assert!(
            text.to_lowercase().contains("daemon"),
            "the fallback warning must name the daemon as the cause, got: {}",
            text
        );
    }

    #[test]
    #[serial]
    fn v5_save_video_on_top_writes_no_static_record() {
        // A skwd native video on top with a resolvable daemon path is an
        // exact video today: no wallpaper.txt, and the packaged video record
        // instead of the absolute video.txt. The static fix must not touch
        // the video arm.
        let media_tmp = TempDir::new().unwrap();
        let video = media_tmp.path().join("slugcat.mp4");
        std::fs::write(&video, b"fake video").unwrap();
        let video_str = video.to_string_lossy().to_string();
        let stub = SaveStub::new(
            &save_layers_doc("skwd-wall-vk", "skwd-wall-vk"),
            None,
            "ok",
            &save_daemon_doc("video", &video_str, &video_str),
            "/pictures/stale/car3.jpg",
        );
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");
        assert!(
            !stub.provider_dir(&theme).join("wallpaper.txt").exists(),
            "exact video on top: no static record may be written"
        );
        assert!(
            !stub
                .provider_dir(&theme)
                .join(wallpaper_authority::PAINTER_VIDEO_FILE)
                .exists(),
            "the absolute video record must not survive a save"
        );
        assert_eq!(
            std::fs::read(
                theme
                    .path()
                    .join("media")
                    .join(crate::theme_media::VIDEO_FILE_NAME)
            )
            .unwrap(),
            b"fake video",
            "the video must travel packaged inside the theme"
        );
        let record = std::fs::read_to_string(
            stub.provider_dir(&theme)
                .join(crate::theme_media::VIDEO_RECORD_FILE),
        )
        .expect("the packaged video record must travel instead");
        assert_eq!(record.trim(), "video=media/background.mp4");
    }

    // ── Theme packages T1: save carries its own background ────────────
    //
    // WHY these exist: a copied theme folder must travel with its look. The
    // background is packaged under `{theme}/media/` and named by a
    // theme-relative `poster.txt` record, and the absolute wallpaper/video
    // records do NOT survive the save (U2 — they name this machine).
    // Packaging is best-effort: a broken video or a missing ffmpeg can never
    // fail a save.
    fn ffmpeg_available() -> bool {
        std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn synth_video(path: &Path) -> bool {
        std::process::Command::new("ffmpeg")
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=5:size=320x240:rate=10",
                "-y",
            ])
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    #[serial]
    fn v5_save_packages_static_wallpaper_into_the_theme() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("joker.png");
        let bytes = b"static-image-bytes-kept-verbatim".to_vec();
        std::fs::write(&src, &bytes).unwrap();
        let src_str = src.to_string_lossy().to_string();
        let stub = SaveStub::new(
            &save_layers_doc("skwd-paper", "skwd-paper"),
            None,
            "ok",
            &save_daemon_doc("static", &src_str, &src_str),
            &src_str,
        );
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");

        let poster = theme.path().join("media").join("poster.png");
        assert!(
            poster.exists(),
            "save must package the static wallpaper into media/"
        );
        assert_eq!(
            std::fs::read(&poster).unwrap(),
            bytes,
            "the packaged static image must keep its bytes"
        );
        let record = std::fs::read_to_string(stub.provider_dir(&theme).join("poster.txt"))
            .expect("save must write the theme-relative poster record");
        assert_eq!(
            record.trim(),
            "poster=media/poster.png",
            "the record must name the file that was actually written"
        );
        // Report evidence (visible with `--nocapture`): the packaged layout.
        println!(
            "[evidence] saved theme media: {} ({} bytes) | record: {}",
            poster.display(),
            std::fs::metadata(&poster).unwrap().len(),
            record.trim()
        );
        assert!(
            stub.wallpaper_txt(&theme).is_none(),
            "the absolute wallpaper record must not survive a save: the packaged poster travels instead"
        );
    }

    #[test]
    #[serial]
    fn v5_save_video_on_top_packages_a_native_jpeg_poster() {
        if !ffmpeg_available() {
            println!("ffmpeg not available — skipping video poster save test");
            return;
        }
        let media = TempDir::new().unwrap();
        let video = media.path().join("slugcat.mp4");
        assert!(synth_video(&video), "failed to synthesize the fixture video");
        let video_str = video.to_string_lossy().to_string();
        let stub = SaveStub::new(
            &save_layers_doc("skwd-wall-vk", "skwd-wall-vk"),
            None,
            "ok",
            &save_daemon_doc("video", &video_str, &video_str),
            "/pictures/stale/car3.jpg",
        );
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");

        let poster = theme.path().join("media").join("poster.jpg");
        assert!(poster.exists(), "a video save must package its poster");
        let bytes = std::fs::read(&poster).unwrap();
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Jpeg,
            "the video poster must be JPEG"
        );
        let decoded = image::ImageReader::open(&poster).unwrap().decode().unwrap();
        assert_eq!(
            (decoded.width(), decoded.height()),
            (320, 240),
            "the poster must keep the video's native resolution"
        );
        let record = std::fs::read_to_string(stub.provider_dir(&theme).join("poster.txt"))
            .expect("a video save must write the theme-relative poster record");
        assert_eq!(record.trim(), "poster=media/poster.jpg");
        // Report evidence (visible with `--nocapture`): the packaged layout.
        println!(
            "[evidence] saved theme media: {} ({} bytes, {}x{}) | record: {}",
            poster.display(),
            bytes.len(),
            decoded.width(),
            decoded.height(),
            record.trim()
        );
        let painter = stub
            .provider_dir(&theme)
            .join(wallpaper_authority::PAINTER_VIDEO_FILE);
        assert!(
            !painter.exists(),
            "the absolute painter video record must not survive a save: the packaged video travels instead"
        );
        let packaged = std::fs::read_to_string(
            stub.provider_dir(&theme)
                .join(crate::theme_media::VIDEO_RECORD_FILE),
        )
        .expect("a video save must record the packaged video");
        assert_eq!(packaged.trim(), "video=media/background.mp4");
    }

    #[test]
    #[serial]
    fn v5_save_survives_a_video_whose_poster_cannot_be_extracted() {
        let media = TempDir::new().unwrap();
        let video = media.path().join("broken.mp4");
        std::fs::write(&video, b"definitely not a video").unwrap();
        let video_str = video.to_string_lossy().to_string();
        let stub = SaveStub::new(
            &save_layers_doc("skwd-wall-vk", "skwd-wall-vk"),
            None,
            "ok",
            &save_daemon_doc("video", &video_str, &video_str),
            "/pictures/stale/car3.jpg",
        );
        let theme = TempDir::new().unwrap();
        let res = NoctaliaV5Provider::new().save(theme.path());
        assert!(
            res.is_ok(),
            "a broken video must never fail the save: {res:?}"
        );
        assert!(
            !stub.provider_dir(&theme).join("poster.txt").exists(),
            "no poster file means no poster record"
        );
        assert!(
            !theme.path().join("media").join("poster.jpg").exists(),
            "no extractable frame means no poster artifact"
        );
    }

    /// The correction invariant: `poster.txt` and `media/poster.*` describe
    /// the theme's CURRENT background, or they do not exist. A re-save that
    /// cannot (re)package a background must drop the previously packaged
    /// poster, otherwise apply — which resolves the theme poster FIRST — would
    /// paint the stale image instead of the freshly recorded background.
    #[test]
    #[serial]
    fn v5_resave_without_a_poster_source_drops_the_stale_poster() {
        let fixtures = TempDir::new().unwrap();
        let src = fixtures.path().join("joker.png");
        let bytes = b"first-save-static-bytes".to_vec();
        std::fs::write(&src, &bytes).unwrap();
        let src_str = src.to_string_lossy().to_string();

        let theme = TempDir::new().unwrap();
        let provider_dir = theme.path().join("providers").join("noctalia-v5");
        let media_dir = theme.path().join("media");

        // First save: a static background is packaged into the theme.
        {
            let _stub = SaveStub::new(
                &save_layers_doc("skwd-paper", "skwd-paper"),
                None,
                "ok",
                &save_daemon_doc("static", &src_str, &src_str),
                &src_str,
            );
            NoctaliaV5Provider::new()
                .save(theme.path())
                .expect("first save must succeed");
            assert!(
                provider_dir.join("poster.txt").exists(),
                "the first save must package a poster"
            );
            assert!(
                media_dir.join("poster.png").exists(),
                "the first save must write the packaged poster file"
            );
        }

        // Re-save the SAME theme with no background source at all: the old
        // packaged poster must not survive (apply would paint it theme-first
        // even though it no longer describes the theme's background).
        let stub = SaveStub::new(
            &save_layers_doc("mystery-layer", "mystery-layer"),
            None,
            "ok",
            &save_daemon_doc("static", "/nonexistent/gone.png", "/nonexistent/gone.png"),
            "",
        );
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("the re-save must still succeed");

        assert!(
            !provider_dir.join("poster.txt").exists(),
            "a re-save with no poster source must remove the stale poster record"
        );
        let strays: Vec<String> = std::fs::read_dir(&media_dir)
            .map(|it| {
                it.flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| n.starts_with("poster."))
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            strays.is_empty(),
            "a re-save with no poster source must remove every media/poster.* file, found {strays:?}"
        );

        // The absolute record does not survive a save either (U2): with no
        // packaged poster left the theme owns no background, so apply must
        // paint nothing instead of resurrecting a stale image.
        assert!(
            stub.wallpaper_txt(&theme).is_none(),
            "the absolute wallpaper record must not survive a save"
        );
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {res:?}");
        let log = stub.logged();
        assert!(
            !log.contains("wallpaper-set"),
            "apply must not paint a background the theme no longer owns, log was:\n{log}"
        );
        assert!(
            !log.contains("media/poster"),
            "apply must not paint a stale packaged poster, log was:\n{log}"
        );
        std::thread::sleep(Duration::from_millis(500));
    }

    /// The invariant's other half: a successful save with NO background
    /// source at all leaves no poster behind.
    #[test]
    #[serial]
    fn v5_save_without_a_poster_source_leaves_no_poster() {
        let stub = SaveStub::new(
            &save_layers_doc("mystery-layer", "mystery-layer"),
            None,
            "ok",
            &save_daemon_doc("static", "/nonexistent/gone.png", "/nonexistent/gone.png"),
            "",
        );
        let theme = TempDir::new().unwrap();
        let res = NoctaliaV5Provider::new().save(theme.path());
        assert!(res.is_ok(), "save must succeed: {res:?}");
        assert!(
            !stub.provider_dir(&theme).join("poster.txt").exists(),
            "no background source means no poster record"
        );
        let media_dir = theme.path().join("media");
        if media_dir.exists() {
            let stray = std::fs::read_dir(&media_dir)
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("poster."));
            assert!(!stray, "no background source means no media/poster.*");
        }
    }

    // ── Theme packages T3: save packages the video when it fits ───────
    //
    // WHY these exist: a theme may carry its video, but only when that is
    // sensible. An under-bound video is copied into the theme as
    // `media/background.mp4` and named by a theme-relative record; an
    // over-bound video is never copied — the url its manifest already knows
    // is recorded instead. No url means the theme travels with its poster.
    // Packaging is best-effort: a failed copy can never fail a save.

    /// Set the packaging bound for the duration of `f`, restoring the previous
    /// value even on panic. The seam lets the size decision be exercised with
    /// tiny files instead of a real 95 MB video.
    fn with_video_bound<T>(bound: &str, f: impl FnOnce() -> T) -> T {
        struct Restore(Option<String>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(v) => std::env::set_var("HVE_MAX_PACKAGED_VIDEO_BYTES", v),
                    None => std::env::remove_var("HVE_MAX_PACKAGED_VIDEO_BYTES"),
                }
            }
        }
        let _restore = Restore(std::env::var("HVE_MAX_PACKAGED_VIDEO_BYTES").ok());
        std::env::set_var("HVE_MAX_PACKAGED_VIDEO_BYTES", bound);
        f()
    }

    #[test]
    #[serial]
    fn v5_save_packages_an_under_bound_video_into_the_theme() {
        if !ffmpeg_available() {
            println!("ffmpeg not available — skipping under-bound video save test");
            return;
        }
        let media = TempDir::new().unwrap();
        let video = media.path().join("slugcat.mp4");
        assert!(synth_video(&video), "failed to synthesize the fixture video");
        let video_str = video.to_string_lossy().to_string();
        let stub = SaveStub::new(
            &save_layers_doc("skwd-wall-vk", "skwd-wall-vk"),
            None,
            "ok",
            &save_daemon_doc("video", &video_str, &video_str),
            "/pictures/stale/car3.jpg",
        );
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");

        let packaged = theme.path().join("media").join(crate::theme_media::VIDEO_FILE_NAME);
        assert!(
            packaged.exists(),
            "an under-bound video must be copied into the theme"
        );
        let record =
            std::fs::read_to_string(stub.provider_dir(&theme).join(crate::theme_media::VIDEO_RECORD_FILE))
                .expect("save must write the theme-relative video record");
        assert_eq!(record.trim(), "video=media/background.mp4");
        assert!(
            !stub
                .provider_dir(&theme)
                .join(wallpaper_authority::PAINTER_VIDEO_FILE)
                .exists(),
            "the absolute painter video record must not survive a save"
        );
    }

    #[test]
    #[serial]
    fn v5_save_over_bound_video_records_the_url_instead_of_copying() {
        if !ffmpeg_available() {
            println!("ffmpeg not available — skipping over-bound video save test");
            return;
        }
        let media = TempDir::new().unwrap();
        let video = media.path().join("slugcat.mp4");
        assert!(synth_video(&video), "failed to synthesize the fixture video");
        let video_str = video.to_string_lossy().to_string();
        let stub = SaveStub::new(
            &save_layers_doc("skwd-wall-vk", "skwd-wall-vk"),
            None,
            "ok",
            &save_daemon_doc("video", &video_str, &video_str),
            "/pictures/stale/car3.jpg",
        );
        let theme = TempDir::new().unwrap();
        let dir = stub.provider_dir(&theme);
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = format!(
            r#"{{"version":1,"assignments":{{"*":{{"filename":"slugcat.mp4","local_path":"{video_str}","url":"https://example.com/slugcat.mp4"}}}}}}"#
        );
        std::fs::write(dir.join(crate::providers::bg_info::MANIFEST_FILE), manifest).unwrap();

        let res = with_video_bound("1", || NoctaliaV5Provider::new().save(theme.path()));
        assert!(res.is_ok(), "save must succeed: {res:?}");

        assert!(
            !theme
                .path()
                .join("media")
                .join(crate::theme_media::VIDEO_FILE_NAME)
                .exists(),
            "an over-bound video must never be copied"
        );
        let record =
            std::fs::read_to_string(dir.join(crate::theme_media::VIDEO_RECORD_FILE))
                .expect("the url must be recorded when the video is over the bound");
        assert!(
            record.contains("url=https://example.com/slugcat.mp4"),
            "the record must carry the manifest's url, was: {record}"
        );
    }

    #[test]
    #[serial]
    fn v5_save_over_bound_video_without_a_url_records_nothing() {
        // No ffmpeg needed: the video packaging is independent of the poster.
        let media = TempDir::new().unwrap();
        let video = media.path().join("slugcat.mp4");
        std::fs::write(&video, b"tiny fake video").unwrap();
        let video_str = video.to_string_lossy().to_string();
        let stub = SaveStub::new(
            &save_layers_doc("skwd-wall-vk", "skwd-wall-vk"),
            None,
            "ok",
            &save_daemon_doc("video", &video_str, &video_str),
            "/pictures/stale/car3.jpg",
        );
        let theme = TempDir::new().unwrap();
        let dir = stub.provider_dir(&theme);

        let res = with_video_bound("1", || NoctaliaV5Provider::new().save(theme.path()));
        assert!(res.is_ok(), "save must succeed: {res:?}");

        assert!(
            !theme
                .path()
                .join("media")
                .join(crate::theme_media::VIDEO_FILE_NAME)
                .exists(),
            "an over-bound video with no url must not be copied"
        );
        assert!(
            !dir.join(crate::theme_media::VIDEO_RECORD_FILE).exists(),
            "an over-bound video with no url records nothing"
        );
    }

    #[test]
    #[serial]
    fn v5_save_survives_a_video_that_cannot_be_published() {
        // No ffmpeg needed: the publish failure is exercised by occupying the
        // packaged video name with a directory, so the rename cannot land.
        let media = TempDir::new().unwrap();
        let video = media.path().join("slugcat.mp4");
        std::fs::write(&video, b"tiny fake video").unwrap();
        let video_str = video.to_string_lossy().to_string();
        let stub = SaveStub::new(
            &save_layers_doc("skwd-wall-vk", "skwd-wall-vk"),
            None,
            "ok",
            &save_daemon_doc("video", &video_str, &video_str),
            "/pictures/stale/car3.jpg",
        );
        let theme = TempDir::new().unwrap();
        let dir = stub.provider_dir(&theme);
        std::fs::create_dir_all(
            theme
                .path()
                .join("media")
                .join(crate::theme_media::VIDEO_FILE_NAME),
        )
        .unwrap();

        let res = NoctaliaV5Provider::new().save(theme.path());
        assert!(
            res.is_ok(),
            "a video that cannot be published must never fail the save: {res:?}"
        );
        assert!(
            !dir.join(crate::theme_media::VIDEO_RECORD_FILE).exists(),
            "no published video means no video record"
        );
    }

    // ── Unit 1c2: pure re-assert decision ──
    //
    // WHY these exist: `reassert_colours` (the `set-colours` refresh of the
    // capability-routing contract) must decide from data alone — no I/O, no
    // `Command` — so the decision is pinned here and the method stays a thin
    // shell around it.

    #[test]
    fn palette_needs_reassert_match_returns_false() {
        let snapshot = br#"{"palette":"blue"}"#;
        assert!(
            !palette_needs_reassert(
                Some("custom theme-blue"),
                Some(snapshot),
                snapshot,
                "theme-blue"
            ),
            "live scheme and bytes both match the snapshot: nothing to do"
        );
    }

    #[test]
    fn palette_needs_reassert_engine_override_returns_true() {
        let snapshot = br#"{"palette":"wall"}"#;
        let live = br#"{"palette":"blue"}"#;
        assert!(
            palette_needs_reassert(Some("custom skwd-wall"), Some(live), snapshot, "theme-blue"),
            "another backend's scheme with different bytes must re-assert"
        );
    }

    #[test]
    fn palette_needs_reassert_same_scheme_changed_bytes_returns_true() {
        let snapshot = br#"{"palette":"blue-v2"}"#;
        let live = br#"{"palette":"blue-v1"}"#;
        assert!(
            palette_needs_reassert(Some("custom theme-blue"), Some(live), snapshot, "theme-blue"),
            "right scheme but drifted bytes must re-assert"
        );
    }

    #[test]
    fn palette_needs_reassert_unreadable_scheme_reasserts() {
        // `None` = noctalia not answering / no scheme readable. Pinned
        // `true`: the re-assert is idempotent (copy + set + templates
        // converge on the snapshot), so an unreadable scheme cannot start a
        // fight; a dead daemon surfaces as `Err` from the CLI step instead.
        let snapshot = br#"{"palette":"blue"}"#;
        assert!(
            palette_needs_reassert(None, Some(snapshot), snapshot, "theme-blue"),
            "an unreadable live scheme must re-assert, never silently keep"
        );
    }

    #[test]
    fn palette_needs_reassert_empty_snapshot_or_name_returns_false() {
        assert!(
            !palette_needs_reassert(Some("custom skwd-wall"), Some(b"x"), b"", "theme-blue"),
            "an empty snapshot owns nothing"
        );
        assert!(
            !palette_needs_reassert(
                Some("custom skwd-wall"),
                Some(br#"{"palette":"blue"}"#),
                br#"{"palette":"blue"}"#,
                ""
            ),
            "an empty name owns nothing"
        );
    }

    #[test]
    fn palette_needs_reassert_trims_scheme_whitespace() {
        // CLI output carries a trailing newline; the caller-facing decision
        // trims it so a matching scheme is not misread as drift.
        let snapshot = br#"{"palette":"blue"}"#;
        assert!(
            !palette_needs_reassert(
                Some("custom theme-blue\n"),
                Some(snapshot),
                snapshot,
                "theme-blue"
            ),
            "a matching scheme with trailing whitespace must not re-assert"
        );
    }

    #[test]
    fn palette_needs_reassert_missing_live_palette_returns_true() {
        // Scheme matches but the live palette file is gone/unreadable: the
        // bytes cannot be proven equal, and the copy recreates the file —
        // a genuine repair, not a fight.
        let snapshot = br#"{"palette":"blue"}"#;
        assert!(
            palette_needs_reassert(Some("custom theme-blue"), None, snapshot, "theme-blue"),
            "a missing live palette must re-assert even when the scheme matches"
        );
    }

    // ── U1 + U5: the pure layout predicate ──
    //
    // WHY these exist: `verify_and_repair_layout` must decide from DATA
    // alone whether Noctalia's re-persist wiped the theme's layout — no
    // I/O, no CLI — so the decision is pinned here and the apply path
    // stays a thin shell around it
    // (`odd/tasks/noctalia-layout-survives-the-apply.md`, U1 + U5 + U3-e).
    // U5 corrected U1 after the verification refuted it: the predicate is
    // SECTION-presence only — the key-presence half reported five false
    // items on the keeper's REAL healthy pair (Noctalia migrates keys
    // inside sections that stay present), repaired a healthy state, and
    // was blind to `[[array-of-tables]]` headers. The repudiated
    // `layout_predicate_reports_a_key_missing_from_a_surviving_section`
    // test went with that half.

    /// A theme's packaged settings.toml carrying every layout family the
    /// predicate enforces, plus the sections Noctalia legitimately
    /// rewrites (`config_version`, `[hooks]`, `[wallpaper]`, `[plugins]`).
    const THEME_LAYOUT: &str = r#"config_version = "1.2"

[hooks]
community_ids = ["abc"]

[wallpaper]
path = "/home/other-machine/wall.png"

[plugins]
enabled = ["noctalia/mpvpaper"]

[bar]
position = "left"

[bar.default]
widgets = ["clock", "weather"]

[desktop_widgets]
enabled = true

[dock]
enabled = true

[shell]
enabled = true

[widget.clock]
format = "%H:%M"
"#;

    /// The live settings.toml the ratchet leaves behind when the
    /// re-persist drops every layout section (the keeper's failing apply
    /// ended at 2471 B with no `[bar]`).
    const WIPED_SETTINGS: &str = r#"config_version = "1.0"

[wallpaper]
path = "/tmp/stale.png"
"#;

    // ── U12: the apply-generation token ──
    //
    // U9 (7a6f1f5) moved the layout verify onto a DETACHED worker so the
    // apply would stop freezing the UI thread — it did. But the worker
    // carried no identity: with the keeper's REAL themes, whose layout
    // SECTION SETS DIFFER (measured — JoKer declares [desktop_widgets],
    // [widget.battery], [shell.screenshot], ... ; Space declares
    // [widget.tray], [widget.workspaces], [widget.volume], ... ; only
    // [bar] is shared), `missing_theme_layout(A, live_of_B)` is
    // NON-EMPTY in both directions on healthy files. So a superseded
    // worker wakes after a NEWER apply already replaced the live file,
    // decides ITS sections are missing from the newer theme's healthy
    // file, appends its own blocks and reloads — the desktop renders the
    // OLD theme's bar under the NEW theme. The synchronous version could
    // not do this (apply and verify were serialized on one thread);
    // moving it off-thread created the hole. The fixtures below mirror
    // that measured asymmetry.

    /// JoKer-like packaged settings.toml (U12): the layout sections
    /// Space does NOT declare, condensed from the keeper's real files
    /// (READ-ONLY measurement).
    const THEME_LAYOUT_A: &str = r#"config_version = "5.0"

[wallpaper]
path = "/here/a.png"

[plugins]
enabled = []

[bar]
position = "left"

[desktop_widgets]
enabled = true

[widget.battery]
interval = 30

[shell.screenshot]
dir = "/shots"
"#;

    /// Space-like packaged settings.toml (U12): a DIFFERENT layout set —
    /// Space carries its own widget set, none of which JoKer declares.
    const THEME_LAYOUT_B: &str = r#"config_version = "5.0"

[wallpaper]
path = "/here/b.png"

[plugins]
enabled = []

[bar]
position = "bottom"

[widget.tray]
hidden = []

[widget.volume]
step = 5

[widget.workspaces]
count = 6
"#;

    /// U3-(e): the legitimate rewrites must still report "in force" — a
    /// changed wallpaper path, a bumped `config_version`, and a live file
    /// where `[hooks]` / `[plugins]` differ from (or are absent vs) the
    /// packaged text. None of those is the layout.
    #[test]
    fn layout_predicate_tolerates_the_legitimate_rewrites() {
        let live = THEME_LAYOUT
            .replace("/home/other-machine/wall.png", "/here/current.png")
            .replace("config_version = \"1.2\"", "config_version = \"2.0\"");
        assert!(
            missing_theme_layout(THEME_LAYOUT, &live).is_empty(),
            "a changed wallpaper path and a bumped config_version must still \
             report the layout in force"
        );
        // A live file carrying ONLY the layout sections — no [hooks], no
        // [plugins], no [wallpaper] — is also in force.
        let layout_only = THEME_LAYOUT
            .split("\n[bar]\n")
            .collect::<Vec<_>>()
            .last()
            .map(|tail| format!("[bar]\n{tail}"))
            .unwrap();
        assert!(
            missing_theme_layout(THEME_LAYOUT, &layout_only).is_empty(),
            "a live file that lost only machine/behaviour sections must still \
             report the layout in force"
        );
    }

    /// A wiped live file must report each missing section ONCE, whole —
    /// never one noisy item per key of a section that is not there.
    #[test]
    fn layout_predicate_reports_wiped_sections_as_whole() {
        assert_eq!(
            missing_theme_layout(THEME_LAYOUT, WIPED_SETTINGS),
            vec![
                "[bar]",
                "[bar.default]",
                "[desktop_widgets]",
                "[dock]",
                "[shell]",
                "[widget.clock]",
            ],
            "every declared layout section missing from the live file must be \
             reported by name, without listing its keys"
        );
    }

    /// U3-(c), pure half: a theme that declares no layout has nothing to
    /// enforce, whatever the live file carries.
    #[test]
    fn layout_predicate_theme_without_layout_is_vacuously_in_force() {
        assert!(
            missing_theme_layout(WIPED_SETTINGS, "").is_empty(),
            "no declared layout means nothing to enforce"
        );
        assert!(
            missing_theme_layout(WIPED_SETTINGS, THEME_LAYOUT).is_empty(),
            "a layoutless theme must not demand anything of the live file"
        );
    }

    /// `[plugins]`, `[hooks]` and `[wallpaper]` are machine/behaviour
    /// state, never layout: a theme carrying only those declares no
    /// layout, and their absence from the live file is not drift.
    #[test]
    fn layout_predicate_never_demands_machine_sections() {
        let machine_only = "config_version = \"1.2\"\n\n\
            [hooks]\ncommunity_ids = []\n\n\
            [plugins]\nenabled = []\n\n\
            [wallpaper]\npath = \"/x.png\"\n";
        assert!(
            missing_theme_layout(machine_only, WIPED_SETTINGS).is_empty(),
            "machine and behaviour sections must never count as layout"
        );
    }

    /// The theme side of the keeper's REAL migrated pair, condensed from
    /// `~/.config/hve/themes/JoKer/providers/noctalia-v5/settings.toml`
    /// (measured 2026-10-11): layout sections in Noctalia's own packaged
    /// spelling, including the two `[[array-of-tables]]` capsule groups.
    const THEME_MIGRATED: &str = r#"config_version = 14

[bar]
order = [ "default", "barra_lateral" ]

    [bar.barra_lateral]
    auto_hide = true
    start = [ "widget" ]

        [[bar.barra_lateral.capsule_group]]
        enabled = true
        id = "g1"
        members = [ "sysmon", "network_rx" ]

        [[bar.barra_lateral.capsule_group]]
        enabled = true
        id = "g2"
        members = [ "cpu", "network_tx" ]

    [bar.default]
    position = "left"
    radius_bottom_right = 20
    radius_top_right = 20
    start = [ "launcher", "clock" ]

[desktop_widgets]
schema_version = 2

[dock]
enabled = true

[shell]
lang = "es"

[widget.clock]
color = "secondary"
scale = 1.3
"#;

    /// The live side of the keeper's REAL migrated pair, condensed from
    /// `~/.local/state/noctalia/settings.toml` — Noctalia's own re-persist
    /// of `THEME_MIGRATED`. The section set is IDENTICAL; Noctalia
    /// migrated five keys inside it (`auto_hide` -> `smart_auto_hide`;
    /// `[bar.default]` dropped `position` / `radius_bottom_right` /
    /// `radius_top_right` for `concave_edge_corners` / `radius`;
    /// `[widget.clock]` dropped `color` for `capsule` / `capsule_fill`)
    /// and re-indented the sub-tables.
    const LIVE_MIGRATED: &str = r#"config_version = 14

[bar]
order = [ "default", "barra_lateral" ]

    [bar.barra_lateral]
    smart_auto_hide = true
    start = [ "widget" ]

        [[bar.barra_lateral.capsule_group]]
        enabled = true
        id = "g1"
        members = [ "sysmon", "network_rx" ]

        [[bar.barra_lateral.capsule_group]]
        enabled = true
        id = "g2"
        members = [ "cpu", "network_tx" ]

    [bar.default]
    concave_edge_corners = false
    radius = 0
    start = [ "launcher", "clock" ]

[desktop_widgets]
schema_version = 2

[dock]
enabled = true

[shell]
lang = "es"

[widget.clock]
capsule = true
capsule_fill = "on_secondary"
scale = 1.3
"#;

    /// U5 (the correction of U1 after the verification refuted it): the
    /// REAL migrated pair measured on the keeper's files must report the
    /// layout IN FORCE. The refuted key-presence predicate reported five
    /// missing items on exactly this pair, repaired a healthy state, and
    /// failed acceptance criterion 4 on the keeper's own data.
    #[test]
    fn layout_predicate_reports_the_real_migrated_pair_in_force() {
        assert!(
            missing_theme_layout(THEME_MIGRATED, LIVE_MIGRATED).is_empty(),
            "the keeper's real migrated pair must report the layout in force"
        );
    }

    /// U5 / LOW of the refutation: `[[array-of-tables]]` headers must be
    /// SEEN. The theme declares the capsule groups, so a live file that
    /// lost them is drift; the old line parser read `[[bar...]]` as a
    /// header name that never matched layout (and let it terminate the
    /// enclosing section), so it saw nothing in either text.
    #[test]
    fn layout_predicate_sees_array_of_tables_sections() {
        let g1 = r#"        [[bar.barra_lateral.capsule_group]]
        enabled = true
        id = "g1"
        members = [ "sysmon", "network_rx" ]

"#;
        let g2 = r#"        [[bar.barra_lateral.capsule_group]]
        enabled = true
        id = "g2"
        members = [ "cpu", "network_tx" ]

"#;
        // BOTH entries must go: one surviving `[[...]]` entry still
        // declares the section (array-of-tables entries are instances of
        // one table, not sections of their own).
        let live_without_groups = LIVE_MIGRATED.replace(g1, "").replace(g2, "");
        assert_eq!(
            missing_theme_layout(THEME_MIGRATED, &live_without_groups),
            vec!["[bar.barra_lateral.capsule_group]"],
            "a declared array-of-tables section missing from the live file must \
             be reported by name, exactly once"
        );
    }

    // ── U6: the surgical repair, pure half ──

    /// U6 (HIGH-2 of the refutation): the repair re-adds ONLY the
    /// missing layout sections. The live text at repair time carries the
    /// keeper's `[plugins]` state (D4 protects it) and the wallpaper /
    /// palette the apply wrote — a verbatim theme copy (the refuted
    /// repair) restores all of that behind the keeper's back. Both
    /// invariants and the byte preservation are pinned here, on texts.
    #[test]
    fn repair_readds_only_missing_sections_and_preserves_every_other_byte() {
        // The live state the repair must find AFTER the apply's later
        // steps: the plugin stayed off, the applied wallpaper persisted,
        // and Noctalia re-persisted everything but the layout sections.
        let live = "config_version = 14\n\n\
            [plugins]\nenabled = []\n\n\
            [wallpaper]\npath = \"/apply/wrote/this.png\"\n\n\
            [theme]\ncustom_palette = \"JokerTheme\"\n";
        let repaired =
            restore_missing_layout_sections(THEME_LAYOUT, live).expect("layout was missing");
        // Every pre-existing byte survives verbatim (the repair appends)...
        assert!(
            repaired.starts_with(live),
            "the repair must leave every pre-existing live byte alone"
        );
        // ...the theme's machine state does NOT come back...
        assert!(
            !repaired.contains("noctalia/mpvpaper"),
            "the repair must never restore the theme's [plugins] list (D4)"
        );
        assert!(
            !repaired.contains("other-machine"),
            "the repair must never revert the wallpaper the apply wrote"
        );
        // ...and the layout is back in force, each section exactly once.
        assert!(
            missing_theme_layout(THEME_LAYOUT, &repaired).is_empty(),
            "the repaired text must carry the theme's layout in force"
        );
        for section in layout_sections(THEME_LAYOUT) {
            assert_eq!(
                repaired.matches(&format!("[{section}]")).count(),
                1,
                "section [{section}] must appear exactly once after the repair"
            );
        }
        // Nothing missing any more -> no second rewrite (bounded repair).
        assert!(
            restore_missing_layout_sections(THEME_LAYOUT, &repaired).is_none(),
            "an in-force layout must produce no further rewrite"
        );
    }

    /// U10 of `odd/tasks/noctalia-layout-survives-the-apply.md` (verified
    /// against the keeper's real files): the repair must restore EVERY
    /// entry of a repeated `[[array-of-tables]]` block. JoKer, Night and
    /// Work carry TWO `[[bar.barra_lateral.capsule_group]]` entries while
    /// `center = [ "group:g1", "ram", "group:g2" ]` references them; the
    /// refuted repair restored 1 of 2, left the `group:g2` reference
    /// dangling, and the presence-based check reported `[]` — so the
    /// apply logged SUCCESS on a half-restored layout.
    #[test]
    fn repair_restores_every_entry_of_a_repeated_block() {
        // A wipe of the WHOLE layout (the ratchet the keeper measured):
        // nothing of the theme's bar survives, both capsule-group entries
        // included. Only the machine/behaviour state is current.
        let live = "config_version = 14\n\n\
            [plugins]\nenabled = []\n\n\
            [wallpaper]\npath = \"/apply/wrote/this.png\"\n";
        let repaired =
            restore_missing_layout_sections(THEME_MIGRATED, live).expect("layout was missing");
        assert_eq!(
            repaired
                .matches("[[bar.barra_lateral.capsule_group]]")
                .count(),
            2,
            "both array-of-tables entries must land, not just the first"
        );
        assert!(
            repaired.contains("id = \"g1\""),
            "the first entry must land"
        );
        assert!(
            repaired.contains("id = \"g2\""),
            "the second entry must land — the keeper's real themes reference it \
             from a `center = [..., \"group:g2\"]` list, and dropping it leaves \
             that reference dangling"
        );
        assert!(
            missing_theme_layout(THEME_MIGRATED, &repaired).is_empty(),
            "the check afterwards must report the layout in force"
        );
        // Nothing it recognised as missing stays missing, so nothing left
        // to rewrite: the repair stays bounded and idempotent.
        assert!(
            restore_missing_layout_sections(THEME_MIGRATED, &repaired).is_none(),
            "an in-force layout must produce no further rewrite"
        );
    }

    // ── Unit 1c2: `reassert_colours` through the stub harness ──
    //
    // The existing `ColorStub` seam (stub `noctalia` on PATH + invocation
    // log) observes CLI calls: set/template counts stay EXACT because
    // nothing else in the suite issues those commands.

    #[test]
    #[serial]
    fn reassert_colours_without_snapshot_is_noop_without_cli() {
        let stub = ColorStub::new("custom skwd-wall", 0, None, true);
        let theme = TempDir::new().unwrap();
        let dir = theme.path().join("providers").join("noctalia-v5");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("source.txt"), "custom JokerTheme").unwrap();
        let res = NoctaliaV5Provider::new().reassert_colours(theme.path());
        assert!(res.is_ok(), "a missing snapshot must be a quiet Ok: {:?}", res);
        assert_eq!(stub.sets(), 0, "no snapshot must issue no set");
        assert_eq!(stub.templates(), 0, "no snapshot must run no templates-apply");
    }

    #[test]
    #[serial]
    fn reassert_colours_non_custom_source_is_noop_without_cli() {
        let stub = ColorStub::new("wallpaper vibrant", 0, None, true);
        let theme = stub.custom_theme("builtin Kanagawa", true);
        let res = NoctaliaV5Provider::new().reassert_colours(theme.path());
        assert!(res.is_ok(), "a non-custom source must be a quiet Ok: {:?}", res);
        assert_eq!(stub.sets(), 0, "a non-custom source must issue no set");
        assert_eq!(stub.templates(), 0, "a non-custom source must run no templates-apply");
    }

    #[test]
    #[serial]
    fn reassert_colours_matching_live_state_writes_nothing() {
        let stub = ColorStub::new("custom JokerTheme", 0, None, true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let snapshot = std::fs::read(
            theme.path().join("providers").join("noctalia-v5").join("palette.json"),
        )
        .unwrap();
        let live_path = PathBuf::from(std::env::var("HVE_NOCTALIA_CONFIG").unwrap())
            .join("palettes")
            .join("JokerTheme.json");
        std::fs::create_dir_all(live_path.parent().unwrap()).unwrap();
        std::fs::write(&live_path, &snapshot).unwrap();
        let res = NoctaliaV5Provider::new().reassert_colours(theme.path());
        assert!(res.is_ok(), "matching state must be Ok: {:?}", res);
        assert_eq!(stub.sets(), 0, "matching state must issue no set");
        assert_eq!(stub.templates(), 0, "matching state must run no templates-apply");
    }

    #[test]
    #[serial]
    fn reassert_colours_yielded_authority_is_quiet_noop() {
        // A hold in flight — opened through the router exactly like the
        // apply path does, never fabricated: the apply's worker owns the
        // palette — Ok with zero writes even though the live state drifted.
        let stub = ColorStub::new("custom skwd-wall", 0, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let hold = background::hold_color_authority()
            .expect("hold is an honest Result")
            .expect("the engine owns the scheme, so the hold must open");
        assert!(
            background::color_authority_held(),
            "precondition: a hold is in flight"
        );
        let res = NoctaliaV5Provider::new().reassert_colours(theme.path());
        // Armed: dropping restores the fake engine config and clears the
        // held state, so this test leaves nothing behind.
        drop(hold);
        assert!(res.is_ok(), "a yielded authority must be a quiet Ok: {:?}", res);
        assert_eq!(stub.sets(), 0, "a yield in flight must issue no set");
        assert_eq!(stub.templates(), 0, "a yield in flight must run no templates-apply");
        assert_eq!(
            std::fs::read_to_string(&stub.skwd_cfg).unwrap(),
            OWNER_JSON,
            "the dropped hold must restore the engine config byte-for-byte"
        );
        assert!(
            !background::color_authority_held(),
            "the dropped hold must clear the held state"
        );
    }

    #[test]
    #[serial]
    fn reassert_colours_through_dyn_provider_reasserts_drift() {
        // Reached through `dyn ThemeProvider` — proof this is the override,
        // not the default no-op (the default would write nothing): a drifted
        // live state converges back on the snapshot with one set + one
        // templates-apply.
        let stub = ColorStub::new("custom skwd-wall", 0, None, true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let live_path = PathBuf::from(std::env::var("HVE_NOCTALIA_CONFIG").unwrap())
            .join("palettes")
            .join("JokerTheme.json");
        std::fs::create_dir_all(live_path.parent().unwrap()).unwrap();
        std::fs::write(&live_path, br#"{"palette":"stale"}"#).unwrap();
        let provider: Box<dyn ThemeProvider> = Box::new(NoctaliaV5Provider::new());
        let res = provider.reassert_colours(theme.path());
        assert!(res.is_ok(), "the re-assert must succeed: {:?}", res);
        assert_eq!(stub.sets(), 1, "one set converges the drifted scheme");
        assert_eq!(stub.templates(), 1, "one templates-apply re-renders");
        let snapshot = std::fs::read(
            theme.path().join("providers").join("noctalia-v5").join("palette.json"),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(&live_path).unwrap(),
            snapshot,
            "the live palette must converge on the snapshot"
        );
        let scheme = std::fs::read_to_string(stub.state_dir.join("scheme")).unwrap();
        assert_eq!(scheme.trim(), "custom JokerTheme");
    }

    // ── U2/U3 + U6/U7: the apply-time layout guard ──
    //
    // The `LayoutStub` seam simulates the ratchet the keeper's journal
    // captured: a stub `noctalia` on PATH whose `config-reload` may wipe
    // the LIVE settings.toml back to the layout-less text, exactly like
    // Noctalia's own re-persist did — so the guard's bounded repair is
    // observed through the real apply path.

    /// The live state AFTER the apply's D4 re-assert and its wallpaper /
    /// palette writes, as Noctalia re-persists it mid-apply: every
    /// layout section is gone, the rest (the plugin the keeper left OFF,
    /// the wallpaper the apply set, the palette selection) is current.
    /// The repair must put the layout back and touch NONE of it.
    const POST_APPLY_WIPED: &str = "config_version = 14\n\n\
        [plugins]\nenabled = []\n\n\
        [wallpaper]\npath = \"/apply/wrote/this.png\"\n\n\
        [theme]\ncustom_palette = \"JokerTheme\"\n";

    struct LayoutStub {
        /// Restored first: `Drop` puts these env values back.
        saved: Vec<(&'static str, Option<String>)>,
        /// The stub `noctalia` bin dir, lives on PATH until restored above.
        _bin: tempfile::TempDir,
        /// Every `msg config-reload` the apply raised, one per line.
        log: PathBuf,
        /// File the stub reads for its wipe mode: `never` / `first` /
        /// `always` (on `config-reload`) / `on-templates` (on
        /// `templates-apply` — the U7 ordering pin).
        mode_file: PathBuf,
        /// Where the stub reads the text it wipes the live file with.
        wipe_src: PathBuf,
        /// When this file exists, the stub's `templates-apply` exits 1 —
        /// U16's failure injection: an apply that fails AFTER the live
        /// settings.toml was written and reloaded.
        templates_fail_flag: PathBuf,
        /// The live settings.toml the stub may wipe.
        target: PathBuf,
        /// Shared env lock, released last.
        _env: crate::test_utils::TempEnv,
    }

    impl LayoutStub {
        /// The stub child inherits the sandboxed `HOME`, so the live
        /// target is `$HOME/.local/state/noctalia/settings.toml` — the
        /// real `noctalia_state_dir()` resolution, hermetically redirected.
        fn new() -> Self {
            let env = crate::test_utils::TempEnv::new();
            let bin = tempfile::tempdir().expect("stub bin dir");
            let keys = ["PATH", "HVE_LAYOUT_SETTLE_MS"];
            let saved: Vec<(&'static str, Option<String>)> = keys
                .iter()
                .map(|k| (*k, std::env::var(k).ok()))
                .collect();

            let live_dir = PathBuf::from(std::env::var("HOME").unwrap())
                .join(".local")
                .join("state")
                .join("noctalia");
            std::fs::create_dir_all(&live_dir).expect("sandboxed state dir");
            let target = live_dir.join("settings.toml");
            let log = bin.path().join("ipc.log");
            let mode_file = bin.path().join("wipe-mode");
            std::fs::write(&log, b"").unwrap();
            std::fs::write(&mode_file, "never").unwrap();
            let wipe_src = bin.path().join("wiped.toml");
            std::fs::write(&wipe_src, WIPED_SETTINGS).unwrap();
            let templates_fail_flag = bin.path().join("templates-fail");

            let stub = format!(
                "#!/bin/sh\n\
                 printf '%s\\n' \"$*\" >> '{log}'\n\
                 if [ \"$1\" = \"msg\" ] && [ \"$2\" = \"config-reload\" ]; then\n\
                 mode=$(cat '{mode}')\n\
                 if [ \"$mode\" = \"always\" ]; then cp '{src}' '{target}'; fi\n\
                 if [ \"$mode\" = \"first\" ]; then\n\
                 printf 'never' > '{mode}'\n\
                 cp '{src}' '{target}'\n\
                 fi\n\
                 fi\n\
                 if [ \"$1\" = \"msg\" ] && [ \"$2\" = \"templates-apply\" ]; then\n\
                 mode=$(cat '{mode}')\n\
                 if [ \"$mode\" = \"on-templates\" ]; then cp '{src}' '{target}'; fi\n\
                 if [ -f '{fail}' ]; then exit 1; fi\n\
                 fi\n\
                 exit 0\n",
                log = log.display(),
                mode = mode_file.display(),
                src = wipe_src.display(),
                target = target.display(),
                fail = templates_fail_flag.display(),
            );
            let noctalia = bin.path().join("noctalia");
            std::fs::write(&noctalia, stub).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&noctalia, std::fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
            let orig_path = std::env::var("PATH").unwrap_or_default();
            std::env::set_var("PATH", format!("{}:{}", bin.path().display(), orig_path));
            // Tests must not pay the production settle: the stub wipes
            // synchronously inside `config-reload`.
            std::env::set_var("HVE_LAYOUT_SETTLE_MS", "0");
            Self {
                saved,
                _bin: bin,
                log,
                mode_file,
                wipe_src,
                templates_fail_flag,
                target,
                _env: env,
            }
        }

        /// U16: make the stub's `templates-apply` exit 1, so the apply
        /// fails on a LATER step, after the live settings.toml was
        /// written and reloaded.
        fn fail_on_templates(&self) {
            std::fs::write(&self.templates_fail_flag, b"").unwrap();
        }

        /// What the stub's next `config-reload` does to the live file.
        fn wipe_mode(&self, mode: &str) {
            std::fs::write(&self.mode_file, mode).unwrap();
        }

        /// Block until the detached layout-verify worker has finished (its
        /// production settle included), so post-apply assertions observe
        /// its FINAL state. U9 moved the verify off the apply's critical
        /// path, so it runs concurrently with the test body — the same
        /// wait `Drop` performs, exposed so a test can pin the worker's
        /// own outcome. Bounded: the worker exits by itself (one settle +
        /// one bounded repair + one re-check settle), so this can never
        /// hang the suite.
        fn wait_for_verify(&self) {
            let deadline = Instant::now() + Duration::from_secs(20);
            while !layout_verify_workers_idle() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(
                layout_verify_workers_idle(),
                "the layout-verify worker outlived its 20 s deadline"
            );
        }

        /// The text the stub wipes the live file with — tests replace it
        /// to simulate Noctalia re-persisting a DIFFERENT live state.
        fn wipe_source(&self) -> &Path {
            &self.wipe_src
        }

        /// How many `msg config-reload` calls the apply raised: 1 is the
        /// step-0 reload, 2 means the repair fired exactly once.
        fn reloads(&self) -> usize {
            std::fs::read_to_string(&self.log)
                .unwrap_or_default()
                .lines()
                .filter(|l| l.trim() == "msg config-reload")
                .count()
        }

        fn live_settings(&self) -> &Path {
            &self.target
        }

        /// A theme whose `providers/noctalia-v5/settings.toml` carries the
        /// given text — and nothing else, so apply's wallpaper, palette
        /// and video phases stay inert around the step-0 restore.
        fn theme_with(&self, settings: &str) -> TempDir {
            let theme = TempDir::new().unwrap();
            let dir = theme.path().join("providers").join("noctalia-v5");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("settings.toml"), settings).unwrap();
            theme
        }

        /// The same theme plus a `source.txt`, so apply also runs
        /// `color-scheme-set` + `templates-apply` around the layout guard
        /// (the U7 ordering pin).
        fn theme_with_source(&self, settings: &str, source: &str) -> TempDir {
            let theme = self.theme_with(settings);
            let dir = theme.path().join("providers").join("noctalia-v5");
            std::fs::write(dir.join("source.txt"), source).unwrap();
            theme
        }
    }

    impl Drop for LayoutStub {
        fn drop(&mut self) {
            // The detached layout-verify worker (U9) reads PROCESS-GLOBAL
            // state: `PATH` to resolve this stub's `noctalia`,
            // `HVE_LAYOUT_SETTLE_MS` for its settle, the sandboxed `HOME`
            // to reach the live settings.toml — and its bounded repair may
            // still WRITE that file. Keep THIS sandbox installed until the
            // worker has finished — same wait and same loud deadline
            // contract as `ColorStub::drop`, so a worker can never
            // outlive its test and pollute the next sandbox.
            let deadline = Instant::now() + Duration::from_secs(20);
            while !layout_verify_workers_idle() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            if !layout_verify_workers_idle() {
                let msg = "a detached layout-verify worker outlived its 20 s deadline: \
                           its sandbox is about to be restored and the next test can be polluted";
                eprintln!("LayoutStub::drop: {msg}");
                if !std::thread::panicking() {
                    panic!("LayoutStub::drop: {msg}");
                }
            }
            for (k, v) in &self.saved {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    /// U3-(a): a live file whose layout was wiped after the reload must
    /// trigger the repair — one rewrite, one extra reload — and the apply
    /// still succeeds (best-effort, like the D4 re-assert).
    #[test]
    #[serial]
    fn apply_repairs_a_wiped_layout_exactly_once() {
        let stub = LayoutStub::new();
        stub.wipe_mode("first");
        let theme = stub.theme_with(THEME_LAYOUT);

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(
            res.is_ok(),
            "a layout problem must never fail the apply: {res:?}"
        );
        stub.wait_for_verify();
        assert_eq!(
            stub.reloads(),
            2,
            "one step-0 reload plus exactly one repair reload"
        );
        let live = std::fs::read_to_string(stub.live_settings()).unwrap();
        assert!(
            live.starts_with(WIPED_SETTINGS),
            "the repair must re-add the layout without touching a byte of the \
             file it found"
        );
        assert!(
            missing_theme_layout(THEME_LAYOUT, &live).is_empty(),
            "the repair must leave the theme's layout in the live file"
        );
    }

    /// U3-(b): an intact live layout must not be rewritten — the repair's
    /// signature (a second `config-reload`) never appears.
    #[test]
    #[serial]
    fn apply_over_an_intact_layout_repairs_nothing() {
        let stub = LayoutStub::new();
        // Default wipe mode `never`: the healthy apply the keeper's
        // sampler measured, where Noctalia's re-persist keeps the layout.
        let theme = stub.theme_with(THEME_LAYOUT);

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "the healthy apply must succeed: {res:?}");
        stub.wait_for_verify();
        assert_eq!(
            stub.reloads(),
            1,
            "an intact layout must not trigger a repair reload"
        );
        assert_eq!(
            std::fs::read_to_string(stub.live_settings()).unwrap(),
            THEME_LAYOUT,
            "step 0's copy must stand un-repaired"
        );
    }

    /// U3-(c): a theme that declares no layout has nothing to enforce —
    /// even a reload that wipes the live file must not trigger a repair.
    #[test]
    #[serial]
    fn apply_with_a_layoutless_theme_never_repairs() {
        let stub = LayoutStub::new();
        stub.wipe_mode("always");
        let theme = stub.theme_with(WIPED_SETTINGS);

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "a layoutless theme must apply cleanly: {res:?}");
        stub.wait_for_verify();
        assert_eq!(
            stub.reloads(),
            1,
            "no declared layout means no repair reload"
        );
    }

    /// U3-(d): when every reload wipes the file, the repair still stops at
    /// ONE attempt — never a loop — and the apply stays best-effort.
    #[test]
    #[serial]
    fn apply_repair_is_bounded_when_the_rewrite_does_not_stick() {
        let stub = LayoutStub::new();
        stub.wipe_mode("always");
        let theme = stub.theme_with(THEME_LAYOUT);

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(
            res.is_ok(),
            "a repair that cannot stick must never fail the apply: {res:?}"
        );
        stub.wait_for_verify();
        assert_eq!(
            stub.reloads(),
            2,
            "the repair must reload once and never loop"
        );
        assert_eq!(
            std::fs::read_to_string(stub.live_settings()).unwrap(),
            WIPED_SETTINGS,
            "the last wipe stands: no second repair may fire after the \
             bounded re-check"
        );
    }

    /// U6 (HIGH-2 of the refutation): a firing repair must NOT undo D4
    /// nor clobber the apply's own writes. The live state at repair time
    /// (the stub's wipe = Noctalia's re-persist after the D4 re-assert
    /// and the wallpaper/palette writes) carries the plugin the keeper
    /// left OFF and the wallpaper the apply set — the repair re-adds the
    /// layout sections and leaves everything else alone. The refuted
    /// verbatim copy restored the theme's `[plugins] enabled` list and
    /// its stale wallpaper path here.
    #[test]
    #[serial]
    fn apply_repair_keeps_plugins_off_and_keeps_the_apply_writes() {
        let stub = LayoutStub::new();
        std::fs::write(stub.wipe_source(), POST_APPLY_WIPED).unwrap();
        stub.wipe_mode("first");
        let theme = stub.theme_with(THEME_LAYOUT);

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(
            res.is_ok(),
            "a layout problem must never fail the apply: {res:?}"
        );
        stub.wait_for_verify();
        assert_eq!(
            stub.reloads(),
            2,
            "one step-0 reload plus exactly one repair reload"
        );
        let live = std::fs::read_to_string(stub.live_settings()).unwrap();
        assert!(
            live.starts_with(POST_APPLY_WIPED),
            "every byte of the mid-apply live state must survive the repair"
        );
        assert!(
            missing_theme_layout(THEME_LAYOUT, &live).is_empty(),
            "the repair must put the theme's layout back"
        );
        assert!(
            !live.contains("noctalia/mpvpaper"),
            "the repair must never restore the theme's [plugins] list (D4)"
        );
        assert!(
            live.contains("enabled = []"),
            "the keeper's plugin-off state must survive the repair"
        );
        assert!(
            live.contains("path = \"/apply/wrote/this.png\""),
            "the wallpaper the apply wrote must survive the repair"
        );
        assert!(
            !live.contains("other-machine"),
            "the repair must never drag the theme's stale wallpaper back"
        );
    }

    /// U7 (MEDIUM of the refutation): the verify must run at the END of
    /// the apply — the refuted one sat after step 0 while the apply
    /// still performed later writes and reloads (`neutralize` writes the
    /// live file and reloads, `wallpaper-set`, `color-scheme-set`,
    /// `templates-apply`, the async palette reassert), so acceptance
    /// criterion 2 was only sampled early. Here the wipe lands on
    /// `templates-apply`, the LAST synchronous step the apply sends:
    /// only an end-of-apply verify can still catch and repair it.
    #[test]
    #[serial]
    fn apply_verifies_after_the_later_steps() {
        let stub = LayoutStub::new();
        stub.wipe_mode("on-templates");
        let theme = stub.theme_with_source(THEME_LAYOUT, "builtin Kanagawa");

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(
            res.is_ok(),
            "a layout problem must never fail the apply: {res:?}"
        );
        stub.wait_for_verify();
        assert!(
            missing_theme_layout(
                THEME_LAYOUT,
                &std::fs::read_to_string(stub.live_settings()).unwrap()
            )
            .is_empty(),
            "a wipe landing on a LATER step must still be repaired — by the \
             off-thread verify, shortly after the apply returns"
        );
        // Ordering pin: the repair's config-reload must be the LAST line
        // the stub logged — i.e. after `templates-apply`, never before it.
        let log = std::fs::read_to_string(&stub.log).unwrap();
        let templates_at = log
            .find("msg templates-apply")
            .expect("the builtin scheme must reach templates-apply");
        let mut reloads_at = Vec::new();
        let mut offset = 0;
        for line in log.lines() {
            if line.trim() == "msg config-reload" {
                reloads_at.push(offset);
            }
            offset += line.len() + 1;
        }
        assert_eq!(
            reloads_at.len(),
            2,
            "one step-0 reload plus exactly one repair reload, live log:\n{log}"
        );
        assert!(
            reloads_at[1] > templates_at,
            "the verify (and its repair) must run AFTER the apply's last \
             write/reload, live log:\n{log}"
        );
    }

    /// U8-(d) of the correction, RE-PINNED by U14: the layoutless
    /// short-circuit must be GENUINELY pinned — with a non-zero settle
    /// configured, a theme that declares no layout must return without
    /// paying it. U9 moved the settle onto the DETACHED worker, so the
    /// old pin (measuring the APPLY's elapsed time) went VACUOUS: the
    /// apply never paid the settle either way, so deleting the
    /// short-circuit failed no assertion (the independent verification
    /// proved this by construction). The pin now measures the WORKER:
    /// from before the apply until its detached worker is idle. Were
    /// the short-circuit deleted, that worker would sleep the full
    /// 3000 ms settle and this test would see it.
    #[test]
    #[serial]
    fn apply_with_a_layoutless_theme_skips_the_settle() {
        let stub = LayoutStub::new();
        // A settle long enough that "paid it" cannot be confused with
        // test noise, short enough to keep the test fast when it fails.
        std::env::set_var("HVE_LAYOUT_SETTLE_MS", "3000");
        stub.wipe_mode("always");
        let theme = stub.theme_with(WIPED_SETTINGS);

        // U14: measure the WORKER, not the apply — the settle lives on
        // the detached worker since U9, so the apply's own elapsed time
        // pins nothing.
        let started = std::time::Instant::now();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(
            res.is_ok(),
            "a layoutless theme must apply cleanly: {res:?}"
        );
        stub.wait_for_verify();
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(1500),
            "a theme without layout sections must skip the settle entirely — \
             neither the apply nor its detached worker may pay it, but they \
             took {elapsed:?}"
        );
        assert_eq!(
            stub.reloads(),
            1,
            "no declared layout means no repair reload"
        );
    }

    /// U9 of `odd/tasks/noctalia-layout-survives-the-apply.md` — the
    /// regression the keeper FELT ("ahora me parece que algo rasca y
    /// antes era todo fluido"): the theme apply runs SYNCHRONOUSLY ON THE
    /// UI THREAD (Slint callback -> gallery slot `apply_theme` -> theme
    /// manager `apply` -> provider `apply`, no spawn on that path), so
    /// the verify's settle froze the transition for up to ~3 s while the
    /// wallpaper cadence itself (1.85 s) was unchanged. The apply must
    /// return WITHOUT waiting; the verify + bounded repair runs AFTERWARDS
    /// on a detached worker — the same idiom the off-thread colour
    /// re-assert uses. The check must still detect and repair a wipe that
    /// landed on the apply's own steps, only later.
    #[test]
    #[serial]
    fn apply_returns_without_waiting_for_the_layout_verify() {
        let stub = LayoutStub::new();
        // A settle long enough that "waited for it" cannot be confused
        // with test noise, short enough to keep the test fast when it
        // fails. The worker pays it twice (settle + post-repair re-check)
        // off-thread; `wait_for_verify` below absorbs that.
        std::env::set_var("HVE_LAYOUT_SETTLE_MS", "1000");
        stub.wipe_mode("first");
        let theme = stub.theme_with(THEME_LAYOUT);

        let started = std::time::Instant::now();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        let elapsed = started.elapsed();
        assert!(
            res.is_ok(),
            "a layout problem must never fail the apply: {res:?}"
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "the apply must return without waiting for the layout settle \
             (the verify runs off-thread on a spawned worker), but it took {elapsed:?}"
        );
        // The check still runs AFTER the apply returned: the detached
        // worker sees the wipe the step-0 reload left and repairs it.
        stub.wait_for_verify();
        assert_eq!(
            stub.reloads(),
            2,
            "the off-thread verify must still detect and repair the wipe \
             (one step-0 reload plus exactly one repair reload)"
        );
        let live = std::fs::read_to_string(stub.live_settings()).unwrap();
        assert!(
            missing_theme_layout(THEME_LAYOUT, &live).is_empty(),
            "the off-thread repair must leave the theme's layout in the live file"
        );
    }

    /// U12 — the refutation's EXACT scenario, on the keeper's measured
    /// asymmetry: apply A (JoKer-like), then apply B (Space-like) before
    /// A's worker has finished its settle; let BOTH workers complete.
    /// A's worker is superseded: it must yield WITHOUT writing and
    /// WITHOUT reloading, so the live file still carries exactly B's
    /// healthy layout. Without the generation token the superseded
    /// worker wakes, finds A's sections missing from B's healthy file,
    /// appends A's blocks and reloads — B's theme renders A's bar.
    #[test]
    #[serial]
    fn superseded_layout_worker_yields_to_the_newer_apply() {
        let stub = LayoutStub::new();
        // The settle must outlast BOTH applies (each is a handful of
        // stub-CLI spawns, well under 100 ms here), so A's worker only
        // wakes AFTER B's apply replaced the live file — the
        // refutation's timeline (the keeper clicked B 0.4 s after A
        // ended; worker A woke at ~1.5 s). Margin: >10x the applies' cost.
        std::env::set_var("HVE_LAYOUT_SETTLE_MS", "1200");
        // Fixture sanity — the measured asymmetry itself: on HEALTHY
        // files each theme's layout is "missing" from the other's.
        assert!(
            !missing_theme_layout(THEME_LAYOUT_A, THEME_LAYOUT_B).is_empty(),
            "fixture must mirror the real asymmetry: A's layout is not in B's file"
        );
        assert!(
            !missing_theme_layout(THEME_LAYOUT_B, THEME_LAYOUT_A).is_empty(),
            "fixture must mirror the real asymmetry: B's layout is not in A's file"
        );

        let theme_a = stub.theme_with(THEME_LAYOUT_A);
        let res = NoctaliaV5Provider::new().apply(theme_a.path());
        assert!(res.is_ok(), "apply A must succeed: {res:?}");
        let theme_b = stub.theme_with(THEME_LAYOUT_B);
        let res = NoctaliaV5Provider::new().apply(theme_b.path());
        assert!(res.is_ok(), "apply B must succeed: {res:?}");

        stub.wait_for_verify();
        assert_eq!(
            stub.reloads(),
            2,
            "only the two applies' step-0 reloads: A's superseded worker \
             must not fire a repair reload"
        );
        assert_eq!(
            std::fs::read_to_string(stub.live_settings()).unwrap(),
            THEME_LAYOUT_B,
            "A's superseded worker must never lay A's layout over B's file — \
             no append, B's step-0 copy must stand byte-for-byte"
        );
    }

    /// U12 control — the healthy case must still work under the
    /// generation token: the CURRENT apply's worker still detects and
    /// repairs a wipe (here B's `templates-apply` wipes the live file),
    /// while the superseded predecessor yields. Three reloads, not four:
    /// A's step-0, B's step-0, B's repair — A's worker adds none.
    #[test]
    #[serial]
    fn current_apply_worker_still_repairs_a_wipe_after_its_predecessor_yielded() {
        let stub = LayoutStub::new();
        // Same margin reasoning as the sibling test above.
        std::env::set_var("HVE_LAYOUT_SETTLE_MS", "1200");
        stub.wipe_mode("on-templates");
        let theme_a = stub.theme_with(THEME_LAYOUT_A);
        let theme_b = stub.theme_with_source(THEME_LAYOUT_B, "builtin Kanagawa");

        let res = NoctaliaV5Provider::new().apply(theme_a.path());
        assert!(res.is_ok(), "apply A must succeed: {res:?}");
        let res = NoctaliaV5Provider::new().apply(theme_b.path());
        assert!(res.is_ok(), "apply B must succeed: {res:?}");

        stub.wait_for_verify();
        assert_eq!(
            stub.reloads(),
            3,
            "A's superseded worker must yield (no reload); B's current worker \
             repairs the wipe with exactly one — live log:\n{}",
            std::fs::read_to_string(&stub.log).unwrap_or_default()
        );
        let live = std::fs::read_to_string(stub.live_settings()).unwrap();
        assert!(
            missing_theme_layout(THEME_LAYOUT_B, &live).is_empty(),
            "the CURRENT apply's worker must still detect and repair the wipe"
        );
    }

    /// U13 — the worker's repair write shares the live settings.toml
    /// path with OTHER writers that can touch it at the same time (the
    /// apply's own `fs::copy`, the rotation neutralizer's `fs::write`,
    /// the keeper's watchdog): it must never truncate the shared file IN
    /// PLACE — a reader racing that truncate can see a torn file.
    /// Publish through a temporary sibling + rename instead (the
    /// `copy_preset` idiom): every reader then sees either the old file
    /// or the complete new one. Pinned by INODE: an in-place
    /// `fs::write` keeps the inode, a rename replaces it.
    #[test]
    #[serial]
    fn layout_repair_publishes_the_live_file_atomically() {
        use std::os::unix::fs::MetadataExt;
        let stub = LayoutStub::new();
        // Settle long enough that the inode is captured while the
        // worker is still asleep, short enough to keep the test fast.
        std::env::set_var("HVE_LAYOUT_SETTLE_MS", "1000");
        stub.wipe_mode("first");
        let theme = stub.theme_with(THEME_LAYOUT);

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(
            res.is_ok(),
            "a layout problem must never fail the apply: {res:?}"
        );
        // The worker is asleep for its settle: capture the inode the
        // apply's copy + the stub's wipe left behind.
        let before = std::fs::metadata(stub.live_settings())
            .expect("the live settings.toml exists after the apply")
            .ino();
        stub.wait_for_verify();
        let after = std::fs::metadata(stub.live_settings())
            .expect("the live settings.toml exists after the repair");
        assert_ne!(
            after.ino(),
            before,
            "the repair must publish through a temporary sibling + rename (the \
             inode must be replaced), never truncate the shared file in place"
        );
        let live = std::fs::read_to_string(stub.live_settings()).unwrap();
        assert!(
            missing_theme_layout(THEME_LAYOUT, &live).is_empty(),
            "the atomic repair must still leave the theme's layout in force"
        );
        let tmp = stub.live_settings().with_file_name(".part-settings.toml");
        assert!(
            !tmp.exists(),
            "the temporary sibling must be renamed away, never left behind"
        );
        let leftover = std::fs::read_dir(stub.live_settings().parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with(".part-"));
        assert!(
            !leftover,
            "no temporary sibling of ANY name may be left behind — with U17 \
             the published name is unique per invocation, so a leftover \
             would be invisible to the old constant-name scan"
        );
    }

    /// U16 (residue (e) of the contract): a FAILED apply must not cancel
    /// its predecessor's repair without inheriting the duty. Apply A
    /// wipes the live layout and its repair worker starts settling;
    /// apply B then fails on an early path BEFORE it touched the live
    /// file (no provider data). B must never supersede A's worker: the
    /// worker sees itself as still current and repairs A's wipe.
    #[test]
    #[serial]
    fn a_failed_early_apply_does_not_supersede_its_predecessors_repair() {
        let stub = LayoutStub::new();
        // The settle must outlast apply B's whole (failing) run so the
        // generation state is settled BEFORE worker A wakes — the same
        // margin reasoning as the U12 sibling tests.
        std::env::set_var("HVE_LAYOUT_SETTLE_MS", "1200");
        stub.wipe_mode("first");
        let theme_a = stub.theme_with(THEME_LAYOUT);

        let res_a = NoctaliaV5Provider::new().apply(theme_a.path());
        assert!(res_a.is_ok(), "apply A must succeed: {res_a:?}");

        // Apply B is honest about failing: a theme dir with NO provider
        // data makes apply return on its first check, before any write.
        let theme_b = tempfile::TempDir::new().unwrap();
        let res_b = NoctaliaV5Provider::new().apply(theme_b.path());
        assert!(
            res_b.is_err(),
            "a theme without provider data must fail to apply: {res_b:?}"
        );

        stub.wait_for_verify();
        assert_eq!(
            stub.reloads(),
            2,
            "A's failed-but-early follower must NOT supersede A: the step-0 \
             reload plus exactly one repair reload — live log:\n{}",
            std::fs::read_to_string(&stub.log).unwrap_or_default()
        );
        let live = std::fs::read_to_string(stub.live_settings()).unwrap();
        assert!(
            missing_theme_layout(THEME_LAYOUT, &live).is_empty(),
            "A's wipe must still be repaired when the follower failed \
             before touching the live file"
        );
    }

    /// U16 (residue (e) of the contract), second half: an apply that ALREADY
    /// wrote the live settings.toml and then fails on a later step must
    /// still spawn the verify — the failure must not strand the live file
    /// unchecked. Here the injected `templates-apply` failure lands AFTER
    /// step 0's copy + reload (which wiped the layout): the worker must
    /// still run and repair.
    #[test]
    #[serial]
    fn a_failed_late_apply_still_spawns_the_layout_verify() {
        let stub = LayoutStub::new();
        // The step-0 reload wipes the layout (the ratchet), and the
        // injected `templates-apply` failure then strands the apply on a
        // later step. The verify must still run and repair.
        stub.wipe_mode("first");
        stub.fail_on_templates();
        let theme = stub.theme_with_source(THEME_LAYOUT, "builtin Kanagawa");

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(
            res.is_err(),
            "the injected templates-apply failure must fail the apply: {res:?}"
        );
        stub.wait_for_verify();
        assert_eq!(
            stub.reloads(),
            2,
            "the live file was written and reloaded, so the verify must run \
             even though the apply returned Err: one step-0 reload plus \
             exactly one repair reload — live log:\n{}",
            std::fs::read_to_string(&stub.log).unwrap_or_default()
        );
        let live = std::fs::read_to_string(stub.live_settings()).unwrap();
        assert!(
            missing_theme_layout(THEME_LAYOUT, &live).is_empty(),
            "a failed apply must not strand the live file unchecked: the \
             spawned worker must still repair the wipe"
        );
    }

    /// U17 (residue (f) of the contract): the repair's temporary sibling
    /// used to be the CONSTANT `.part-settings.toml`, shared by every
    /// worker — two workers inside the atomic write could delete/rename
    /// each other's in-flight temp and publish a partial file. The
    /// sibling name is now unique per PROCESS (pid), per APPLY
    /// (generation) and per INVOCATION (monotonic sequence).
    #[test]
    fn the_atomic_publish_temporary_sibling_is_unique_per_invocation() {
        let base = std::path::Path::new("/state/noctalia/settings.toml");
        let a = atomic_temp_sibling(base, "settings.toml", 7);
        let b = atomic_temp_sibling(base, "settings.toml", 7);
        let c = atomic_temp_sibling(base, "settings.toml", 8);
        assert_ne!(
            a, b,
            "two invocations must never share the sibling, even with the \
             same generation (two repair workers cannot collide)"
        );
        assert_ne!(a, c, "a different apply generation must change the name");
        assert_ne!(
            a,
            base.with_file_name(".part-settings.toml"),
            "the constant sibling name is the residue: every worker shared it"
        );
        assert!(
            a.to_string_lossy()
                .contains(&std::process::id().to_string()),
            "the sibling must be unique per process too: two HVE processes \
             could run a repair at the same wall-clock moment"
        );
    }

    // ── U18: the post-repair re-check must re-test the generation ──
    //
    // The re-check is LOG-ONLY (no write, no reload after it), so the
    // honest pin needs the worker's LOG TEXT. A thread-local subscriber
    // (`set_default` — the repo's `capture_logs` idiom) does NOT reach a
    // spawned thread (verified in tracing-core: set_default installs a
    // thread-local dispatcher; a fresh worker falls back to the GLOBAL
    // default), so these tests install a process-wide capture subscriber
    // ONCE, and each test clears + reads the shared buffer around its
    // own window. All tests that touch the layout worker are `#[serial]`,
    // and only they can emit the pinned lines, so an absence assertion
    // inside the cleared window sees exactly this test's worker.

    /// The shared in-memory tracing sink (see above for why it must be
    /// process-wide, not thread-local).
    #[derive(Clone, Default)]
    struct GlobalLogCapture {
        buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl std::io::Write for GlobalLogCapture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.buf.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for GlobalLogCapture {
        type Writer = GlobalLogCapture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    static GLOBAL_LOG_CAPTURE: std::sync::OnceLock<GlobalLogCapture> =
        std::sync::OnceLock::new();

    fn install_global_log_capture() -> GlobalLogCapture {
        GLOBAL_LOG_CAPTURE.get_or_init(|| {
            let capture = GlobalLogCapture::default();
            // Best-effort: if a global default is already set (it never is
            // in this suite), capture is degraded, never failing a test.
            let _ = tracing::subscriber::set_global_default(
                tracing_subscriber::fmt()
                    .with_writer(capture.clone())
                    .with_ansi(false)
                    .without_time()
                    .with_max_level(tracing::Level::INFO)
                    .finish(),
            );
            capture
        })
        .clone()
    }

    fn u18_read_capture(capture: &GlobalLogCapture) -> String {
        String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string()
    }

    fn u18_clear_capture(capture: &GlobalLogCapture) {
        capture.buf.lock().unwrap().clear();
    }

    /// U18 (residue (g) of the contract): the post-repair re-check must
    /// re-test the generation BEFORE comparing anything. Apply A repairs
    /// its wipe; while worker A is inside its post-repair settle, apply
    /// B (a different theme — the measured section-set asymmetry) writes
    /// the live file. Worker A must NOT then compare A's OLD sections
    /// against B's healthy file (that logs the false diagnostic "STILL
    /// missing after the single repair" against a healthy file — the
    /// diagnostic lie the residue describes); it must yield with the
    /// honest superseded message and leave B's file untouched.
    #[test]
    #[serial]
    fn a_worker_superseded_in_its_post_repair_settle_skips_the_misleading_recheck() {
        let capture = install_global_log_capture();
        u18_clear_capture(&capture);
        let stub = LayoutStub::new();
        // The settle must outlast both applies' whole cost (each is a
        // handful of stub-CLI spawns, well under 100 ms here) so both
        // applies land inside worker A's post-repair settle — the same
        // margin reasoning as the U12 sibling tests.
        std::env::set_var("HVE_LAYOUT_SETTLE_MS", "1200");
        // Fixture sanity — A's layout is NOT in B's healthy file, so the
        // absent re-check would really log the false diagnostic here.
        assert!(
            !missing_theme_layout(THEME_LAYOUT_A, THEME_LAYOUT_B).is_empty(),
            "fixture must mirror the real asymmetry for the re-check to bite"
        );
        stub.wipe_mode("first");
        let theme_a = stub.theme_with(THEME_LAYOUT_A);
        let res = NoctaliaV5Provider::new().apply(theme_a.path());
        assert!(res.is_ok(), "apply A must succeed: {res:?}");

        // Wait until A's repair FIRED (the second reload): worker A is
        // then inside its post-repair settle — B's apply must be picked
        // up by the GENERATION RE-CHECK, not by the file comparison.
        let deadline = Instant::now() + Duration::from_secs(20);
        while stub.reloads() < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            stub.reloads() >= 2,
            "worker A never fired its repair reload — live log:\n{}",
            std::fs::read_to_string(&stub.log).unwrap_or_default()
        );

        let theme_b = stub.theme_with(THEME_LAYOUT_B);
        let res = NoctaliaV5Provider::new().apply(theme_b.path());
        assert!(res.is_ok(), "apply B must succeed: {res:?}");

        stub.wait_for_verify();
        assert_eq!(
            stub.reloads(),
            3,
            "A's step-0 + A's repair + B's step-0: neither worker may add a \
             reload — live log:\n{}",
            std::fs::read_to_string(&stub.log).unwrap_or_default()
        );
        assert_eq!(
            std::fs::read_to_string(stub.live_settings()).unwrap(),
            THEME_LAYOUT_B,
            "B's healthy file must stand byte-for-byte"
        );
        let text = u18_read_capture(&capture);
        assert!(
            !text.contains("STILL missing after the single repair"),
            "the false diagnostic must never be logged against B's healthy \
             file — live capture:\n{text}"
        );
        assert!(
            text.contains("skipping the post-repair re-check"),
            "the superseded worker must log the honest superseded message \
             instead — live capture:\n{text}"
        );
    }

    /// U18 control: the still-current case keeps its honest re-check —
    /// with every reload re-wiping, the ONE bounded repair logs that the
    /// layout is STILL missing. Removing the U18 re-check must not be
    /// done by removing the re-check itself.
    #[test]
    #[serial]
    fn a_current_worker_still_runs_the_post_repair_recheck() {
        let capture = install_global_log_capture();
        u18_clear_capture(&capture);
        let stub = LayoutStub::new();
        stub.wipe_mode("always");
        let theme = stub.theme_with(THEME_LAYOUT);

        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(
            res.is_ok(),
            "a repair that cannot stick must never fail the apply: {res:?}"
        );
        stub.wait_for_verify();
        assert_eq!(
            stub.reloads(),
            2,
            "one step-0 reload plus exactly one repair reload"
        );
        let text = u18_read_capture(&capture);
        assert!(
            text.contains("STILL missing after the single repair"),
            "the still-current worker must keep logging the honest \
             re-check result — live capture:\n{text}"
        );
    }
}
