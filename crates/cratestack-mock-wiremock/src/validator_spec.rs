//! Reads a field's validator attributes (`@length`, `@range`, `@regex`,
//! `@email`, `@uri`, `@iso4217`) into a [`ValidatorSpec`]. Mirrors the
//! argument shapes `cratestack-parser`'s `validator_args.rs` accepts (its
//! helpers are `pub(crate)` there), kept tiny so drift stays reviewable.

use cratestack_core::Field;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ValidatorSpec {
    pub(crate) length: (Option<usize>, Option<usize>),
    pub(crate) range: (Option<i64>, Option<i64>),
    pub(crate) regexes: Vec<String>,
    pub(crate) email: bool,
    pub(crate) uri: bool,
    pub(crate) iso4217: bool,
}

impl ValidatorSpec {
    pub(crate) fn from_field(field: &Field) -> Self {
        let mut spec = Self::default();
        for attribute in &field.attributes {
            let raw = attribute.raw.as_str();
            match attribute_name(raw) {
                "length" => spec.length = bounds(raw, "length").unwrap_or_default(),
                "range" => spec.range = bounds(raw, "range").unwrap_or_default(),
                "regex" => spec.regexes.extend(regex_arg(raw)),
                "email" => spec.email = true,
                "uri" => spec.uri = true,
                "iso4217" => spec.iso4217 = true,
                _ => {}
            }
        }
        spec
    }

    /// The pattern a raw `@regex(...)` attribute carries.
    pub(crate) fn regex_for(&self, raw: &str) -> Option<&str> {
        let pattern = regex_arg(raw)?;
        self.regexes
            .iter()
            .find(|p| **p == pattern)
            .map(String::as_str)
    }

    pub(crate) fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn attribute_name(raw: &str) -> &str {
    let body = raw.strip_prefix('@').unwrap_or(raw);
    body.split('(').next().unwrap_or(body).trim()
}

fn inner<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    raw.strip_prefix(&format!("@{name}("))?.strip_suffix(')')
}

fn bounds<T: std::str::FromStr>(raw: &str, name: &str) -> Option<(Option<T>, Option<T>)> {
    let (mut min, mut max) = (None, None);
    for part in inner(raw, name)?.split(',') {
        let Some((key, value)) = part.split_once(':') else {
            continue;
        };
        let parsed = value.trim().parse().ok();
        match key.trim() {
            "min" => min = parsed,
            "max" => max = parsed,
            _ => {}
        }
    }
    Some((min, max))
}

fn regex_arg(raw: &str) -> Option<String> {
    let arg = inner(raw, "regex")?.trim();
    Some(arg.strip_prefix('"')?.strip_suffix('"')?.to_owned())
}
