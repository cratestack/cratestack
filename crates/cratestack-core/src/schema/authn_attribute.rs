//! The procedure attribute `@authn(maxAge: 300, amr: ["hwk"], acr: ["aal2"])`
//! (ADR 0022, cratestack#1244): how recently and how strongly the caller must
//! have authenticated.
//!
//! One reader for every consumer: `cratestack-parser` refuses an `@authn` it
//! does not read, and `cratestack-macros` turns what it reads into the
//! operation's `AuthnRequirement`. Keys are `maxAge` (seconds, 1 to one
//! year), `amr` (methods that must all be present) and `acr` (acceptable
//! classes, one must match); each at most once, at least one.

use super::Attribute;

/// The attribute's name.
pub const AUTHN_ATTRIBUTE: &str = "@authn";

/// The keys `@authn` takes, in the order its error messages list them.
pub const AUTHN_KEYS: [&str; 3] = ["maxAge", "amr", "acr"];

/// The largest `maxAge`: one year, in seconds.
pub const AUTHN_MAX_AGE_LIMIT: i64 = 31_536_000;

/// What an `@authn` attribute requires.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedAuthn {
    /// `maxAge`, in seconds.
    pub max_age: Option<i64>,
    /// `amr`: methods that must all be present.
    pub amr: Vec<String>,
    /// `acr`: acceptable classes.
    pub acr: Vec<String>,
}

/// The `@authn` among `attributes`, if any (the first, when a schema
/// repeats it; the parser refuses a repeat).
pub fn authn_attribute(attributes: &[Attribute]) -> Option<&Attribute> {
    attributes.iter().find(|attribute| {
        let rest = attribute.raw.trim().strip_prefix(AUTHN_ATTRIBUTE);
        rest.is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
    })
}

/// Reads a whole `@authn(...)` attribute.
pub fn parse_authn_attribute(raw: &str) -> Result<ParsedAuthn, String> {
    let inner = raw
        .trim()
        .strip_prefix(AUTHN_ATTRIBUTE)
        .and_then(|rest| rest.strip_prefix('('))
        .and_then(|rest| rest.strip_suffix(')'))
        .ok_or_else(|| {
            "`@authn` takes arguments in parentheses, e.g. `@authn(maxAge: 300)`".to_owned()
        })?;
    let mut parsed = ParsedAuthn::default();
    let mut seen: Vec<&str> = Vec::new();
    for argument in split_top_level(inner)? {
        let (key, value) = argument
            .split_once(':')
            .map(|(key, value)| (key.trim(), value.trim()))
            .ok_or_else(|| format!("`@authn` argument `{argument}` is not `key: value`"))?;
        if !AUTHN_KEYS.contains(&key) {
            return Err(format!(
                "`@authn` has no argument `{key}`; it takes {}",
                AUTHN_KEYS.map(|key| format!("`{key}`")).join(", ")
            ));
        }
        if seen.contains(&key) {
            return Err(format!("`@authn` repeats `{key}`"));
        }
        seen.push(key);
        match key {
            "maxAge" => parsed.max_age = Some(max_age(value)?),
            "amr" => parsed.amr = string_list(key, value)?,
            _ => parsed.acr = string_list(key, value)?,
        }
    }
    if seen.is_empty() {
        return Err("`@authn` needs at least one of `maxAge`, `amr`, `acr`".to_owned());
    }
    Ok(parsed)
}

fn max_age(value: &str) -> Result<i64, String> {
    let seconds: i64 = value.parse().map_err(|_| {
        format!("`@authn` `maxAge` must be a whole number of seconds, not `{value}`")
    })?;
    if !(1..=AUTHN_MAX_AGE_LIMIT).contains(&seconds) {
        return Err(format!(
            "`@authn` `maxAge` must be between 1 and {AUTHN_MAX_AGE_LIMIT} seconds, not {seconds}"
        ));
    }
    Ok(seconds)
}

fn string_list(key: &str, value: &str) -> Result<Vec<String>, String> {
    let items = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| format!("`@authn` `{key}` must be a list, e.g. `{key}: [\"hwk\"]`"))?;
    let mut list: Vec<String> = Vec::new();
    for item in split_top_level(items)? {
        let text = unquote(item)
            .ok_or_else(|| format!("`@authn` `{key}` item `{item}` must be a quoted string"))?;
        let plain = !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._:/~+-".contains(c));
        if !plain {
            return Err(format!(
                "`@authn` `{key}` value `{text}` must be letters, digits or `._:/~+-`"
            ));
        }
        if list.iter().any(|seen| seen == text) {
            return Err(format!("`@authn` `{key}` lists `{text}` twice"));
        }
        list.push(text.to_owned());
    }
    if list.is_empty() {
        return Err(format!("`@authn` `{key}` must list at least one value"));
    }
    Ok(list)
}

fn unquote(item: &str) -> Option<&str> {
    ['"', '\'']
        .into_iter()
        .find_map(|quote| item.strip_prefix(quote)?.strip_suffix(quote))
}

/// `text` split at commas outside quotes and brackets, each part trimmed.
/// An empty text is no parts; an empty part between commas is an error.
fn split_top_level(text: &str) -> Result<Vec<&str>, String> {
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut start) = (0usize, None::<char>, 0);
    for (at, c) in text.char_indices() {
        match (quote, c) {
            (Some(open), _) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '[') => depth += 1,
            (None, ']') => {
                depth = depth
                    .checked_sub(1)
                    .ok_or("`@authn` has an unmatched `]`")?
            }
            (None, ',') if depth == 0 => {
                parts.push(text[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() || depth != 0 {
        return Err("`@authn` has an unclosed quote or bracket".to_owned());
    }
    let last = text[start..].trim();
    if !last.is_empty() || !parts.is_empty() {
        parts.push(last);
    }
    if parts.iter().any(|part| part.is_empty()) {
        return Err("`@authn` has an empty argument".to_owned());
    }
    Ok(parts)
}

#[cfg(test)]
#[path = "authn_attribute_tests.rs"]
mod tests;
