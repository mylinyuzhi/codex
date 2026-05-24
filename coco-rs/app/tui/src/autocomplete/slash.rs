//! Slash-command suggestion ranker.
//!
//! Pure function over a `query` plus a snapshot of `SlashCommandInfo`s.
//! TS source: `src/utils/suggestions/commandSuggestions.ts` —
//! `getCommandSuggestions` builds a Fuse.js index but then layers a
//! hand-written priority hierarchy on top so prefix matches always beat
//! fuzzy hits. The Rust port skips Fuse entirely: the registry holds at
//! most ~100 commands and a hand-rolled ranker matches TS behaviour more
//! tightly than a fuzzy library would.
//!
//! Priority (lower number = better):
//! 1. Exact name match
//! 2. Exact alias match
//! 3. Name starts with `query`
//! 4. Any alias starts with `query`
//! 5. Name contains `query`
//! 6. Any alias contains `query`
//! 7. Name is a subsequence of `query` (typo-friendly catch-all, e.g.
//!    `/clr` still finds `/clear`)
//!
//! Within a priority bucket, shorter names sort first so `/help` outranks
//! `/help-extra` when the user types `/h`. The empty query returns every
//! command in original (registry) order — that is the listing view shown
//! the instant the user types `/` with nothing after it.

use crate::state::SlashCommandInfo;
use crate::widgets::suggestion_popup::SuggestionItem;

/// Rank a snapshot of slash commands against `query` and return popup
/// items in display order. An empty `query` keeps registry order so the
/// initial `/` keystroke shows the full catalogue.
pub fn rank(query: &str, commands: &[SlashCommandInfo]) -> Vec<SuggestionItem> {
    if query.is_empty() {
        return commands.iter().map(to_item).collect();
    }
    let needle = query.to_lowercase();
    let mut scored: Vec<(Priority, &SlashCommandInfo)> = commands
        .iter()
        .filter_map(|cmd| classify(&needle, cmd).map(|p| (p, cmd)))
        .collect();
    // Total order so the popup is deterministic across runs even when
    // upstream `available_commands` arrived from a HashMap iteration.
    // Priority bucket → shorter name → name ascending.
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.name.len().cmp(&b.1.name.len()))
            .then_with(|| a.1.name.cmp(&b.1.name))
    });
    scored.into_iter().map(|(_, cmd)| to_item(cmd)).collect()
}

/// Build a popup row from a command snapshot. Description carries the
/// optional `argument_hint` in front of the prose so users see the call
/// signature before they accept.
fn to_item(cmd: &SlashCommandInfo) -> SuggestionItem {
    let description = match (cmd.argument_hint.as_deref(), cmd.description.as_deref()) {
        (Some(hint), Some(desc)) => Some(format!("{hint}  {desc}")),
        (Some(hint), None) => Some(hint.to_string()),
        (None, Some(desc)) => Some(desc.to_string()),
        (None, None) => None,
    };
    SuggestionItem {
        label: format!("/{}", cmd.name),
        description,
        metadata: None,
    }
}

/// Match-quality bucket. Variant order determines popup ordering — the
/// derived `Ord` walks variants top-to-bottom, so keep the highest-priority
/// match at the top of the enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Priority {
    ExactName,
    ExactAlias,
    PrefixName,
    PrefixAlias,
    ContainsName,
    ContainsAlias,
    Subsequence,
}

fn classify(needle: &str, cmd: &SlashCommandInfo) -> Option<Priority> {
    let name = cmd.name.to_lowercase();
    if name == needle {
        return Some(Priority::ExactName);
    }
    let aliases: Vec<String> = cmd.aliases.iter().map(|a| a.to_lowercase()).collect();
    if aliases.iter().any(|a| a == needle) {
        return Some(Priority::ExactAlias);
    }
    if name.starts_with(needle) {
        return Some(Priority::PrefixName);
    }
    if aliases.iter().any(|a| a.starts_with(needle)) {
        return Some(Priority::PrefixAlias);
    }
    if name.contains(needle) {
        return Some(Priority::ContainsName);
    }
    if aliases.iter().any(|a| a.contains(needle)) {
        return Some(Priority::ContainsAlias);
    }
    if is_tight_subsequence(needle, &name)
        || aliases.iter().any(|a| is_tight_subsequence(needle, a))
    {
        return Some(Priority::Subsequence);
    }
    None
}

/// Whether `needle` appears as a non-contiguous subsequence of `haystack`,
/// AND the matched range stays "tight" — the span between the first and
/// last matched character must be at most `2 × needle.len()`. The cap
/// keeps the fallback typo-friendly (`clr` → `clear`, span 5 vs needle 3)
/// while rejecting wildly-spread matches like `abc` → `build-and-clear`.
///
/// Both inputs must already be lowercased.
///
/// TS parity: stands in for Fuse.js `threshold: 0.3`. Fuse penalises
/// matches by edit-distance; the explicit span cap here gives a
/// similar "don't reach too far" guarantee without pulling in a fuzzy
/// library for ~80 commands.
fn is_tight_subsequence(needle: &str, haystack: &str) -> bool {
    let needle_chars: Vec<char> = needle.chars().collect();
    if needle_chars.is_empty() {
        return true;
    }
    let max_span = needle_chars.len().saturating_mul(2);
    let mut idx = 0;
    let mut first_match: Option<usize> = None;
    for (i, h) in haystack.chars().enumerate() {
        if h == needle_chars[idx] {
            let start = *first_match.get_or_insert(i);
            idx += 1;
            if idx == needle_chars.len() {
                // Equivalent to `(i - start + 1) <= max_span` — clippy
                // prefers the rearranged form. Span is inclusive of both
                // endpoints, hence the "<" rather than "<=".
                return i - start < max_span;
            }
        }
    }
    false
}

#[cfg(test)]
#[path = "slash.test.rs"]
mod tests;
