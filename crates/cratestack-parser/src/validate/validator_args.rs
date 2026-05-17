/// Parse `@length(min: N, max: N)` into `(min, max)` with both bounds optional.
pub(crate) fn parse_length_args(raw: &str) -> Result<(Option<u32>, Option<u32>), String> {
    let inner = strip_attribute_parens(raw, "length")?;
    let (mut min, mut max) = (None, None);
    for part in split_kv_args(&inner) {
        let (key, value) = split_kv(&part)?;
        let parsed: u32 = value
            .parse()
            .map_err(|_| format!("@length expects non-negative integer, got `{value}`"))?;
        match key.as_str() {
            "min" => min = Some(parsed),
            "max" => max = Some(parsed),
            other => return Err(format!("@length: unknown argument `{other}`")),
        }
    }
    if let (Some(lo), Some(hi)) = (min, max)
        && lo > hi
    {
        return Err(format!("@length: min ({lo}) must be <= max ({hi})"));
    }
    Ok((min, max))
}

/// Parse `@range(min: N, max: N)` into `(min, max)` with both bounds optional
/// and signed.
pub(crate) fn parse_range_args(raw: &str) -> Result<(Option<i64>, Option<i64>), String> {
    let inner = strip_attribute_parens(raw, "range")?;
    let (mut min, mut max) = (None, None);
    for part in split_kv_args(&inner) {
        let (key, value) = split_kv(&part)?;
        let parsed: i64 = value
            .parse()
            .map_err(|_| format!("@range expects integer, got `{value}`"))?;
        match key.as_str() {
            "min" => min = Some(parsed),
            "max" => max = Some(parsed),
            other => return Err(format!("@range: unknown argument `{other}`")),
        }
    }
    if let (Some(lo), Some(hi)) = (min, max)
        && lo > hi
    {
        return Err(format!("@range: min ({lo}) must be <= max ({hi})"));
    }
    Ok((min, max))
}

/// Parse `@regex("pattern")` into the pattern string. Validates the regex
/// compiles so we fail at schema-load time rather than first request.
pub(crate) fn parse_regex_arg(raw: &str) -> Result<String, String> {
    let inner = strip_attribute_parens(raw, "regex")?;
    let trimmed = inner.trim();
    let stripped = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| "@regex argument must be a quoted string literal".to_owned())?;
    regex::Regex::new(stripped).map_err(|e| format!("@regex pattern is not a valid regex: {e}"))?;
    Ok(stripped.to_owned())
}

fn strip_attribute_parens(raw: &str, name: &str) -> Result<String, String> {
    let prefix = format!("@{name}(");
    let trimmed = raw
        .strip_prefix(&prefix)
        .ok_or_else(|| format!("@{name} attribute is malformed"))?;
    let inner = trimmed
        .strip_suffix(')')
        .ok_or_else(|| format!("@{name} attribute is missing closing paren"))?;
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
    let (key, value) = part
        .split_once(':')
        .ok_or_else(|| format!("expected `key: value`, got `{part}`"))?;
    Ok((key.trim().to_owned(), value.trim().to_owned()))
}
