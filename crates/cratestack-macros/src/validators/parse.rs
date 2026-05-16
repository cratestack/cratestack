//! Parse `@length(...)`, `@range(...)`, `@regex("...")` attributes
//! into structured args. Local re-implementations of the parser
//! argument helpers — keeps this crate from depending on internals of
//! `cratestack-parser`. The shapes are trivial; if they drift we'll
//! lift them into a shared crate.

pub(super) fn parse_length_args(raw: &str) -> Result<(Option<u32>, Option<u32>), String> {
    let inner = strip_attr_parens(raw, "length")?;
    let (mut min, mut max) = (None, None);
    for part in split_kv_args(&inner) {
        let (key, value) = split_kv(&part)?;
        let parsed: u32 = value.parse().map_err(|_| format!("bad u32: {value}"))?;
        match key.as_str() {
            "min" => min = Some(parsed),
            "max" => max = Some(parsed),
            _ => return Err(format!("unknown @length arg: {key}")),
        }
    }
    Ok((min, max))
}

pub(super) fn parse_range_args(raw: &str) -> Result<(Option<i64>, Option<i64>), String> {
    let inner = strip_attr_parens(raw, "range")?;
    let (mut min, mut max) = (None, None);
    for part in split_kv_args(&inner) {
        let (key, value) = split_kv(&part)?;
        let parsed: i64 = value.parse().map_err(|_| format!("bad i64: {value}"))?;
        match key.as_str() {
            "min" => min = Some(parsed),
            "max" => max = Some(parsed),
            _ => return Err(format!("unknown @range arg: {key}")),
        }
    }
    Ok((min, max))
}

pub(super) fn parse_regex_arg(raw: &str) -> Result<String, String> {
    let inner = strip_attr_parens(raw, "regex")?;
    let trimmed = inner.trim();
    let stripped = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| "expected quoted string".to_owned())?;
    Ok(stripped.to_owned())
}

fn strip_attr_parens(raw: &str, name: &str) -> Result<String, String> {
    let prefix = format!("@{name}(");
    let trimmed = raw.strip_prefix(&prefix).ok_or("malformed")?;
    let inner = trimmed.strip_suffix(')').ok_or("missing close paren")?;
    Ok(inner.to_owned())
}

fn split_kv_args(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

fn split_kv(part: &str) -> Result<(String, String), String> {
    let (key, value) = part.split_once(':').ok_or("expected key: value")?;
    Ok((key.trim().to_owned(), value.trim().to_owned()))
}
