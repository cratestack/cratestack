//! The pattern argument of `@regex("…")`, unescaped.
//!
//! A `.cstack` string literal is a double-quoted string with backslash
//! escapes, so the schema author writes `"^\\+[0-9]+$"` to mean the regex
//! `^\+[0-9]+$` (cratestack#1314). Before that fix only the quotes were
//! stripped, so `\\+` reached the regex engine as "one or more
//! backslashes" and valid input was rejected with nothing at generation
//! time pointing at the cause.
//!
//! Every reader of the pattern (the parser's compile check, the macro's
//! generated validator, Studio's write-time check) calls
//! [`parse_regex_attribute`], so they cannot disagree.
//!
//! Recognised escapes: `\\`, `\"`, `\'`, `\n`, `\r`, `\t`. Anything else
//! (`\d`, `\+`, `\.`) is an error naming the fix, not a silent
//! pass-through: a pass-through is what made `"\\+"` and `"\+"` mean
//! different things without anyone being told.

/// Pattern of `@regex("…")` given the attribute's raw text.
pub fn parse_regex_attribute(raw: &str) -> Result<String, String> {
    let inner = raw
        .strip_prefix("@regex(")
        .ok_or("@regex attribute is malformed")?
        .strip_suffix(')')
        .ok_or("@regex attribute is missing closing paren")?;
    let body = inner
        .trim()
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| "@regex argument must be a quoted string literal".to_owned())?;
    unescape_string_body(body)
}

/// The text between the quotes of a double-quoted `.cstack` string.
pub fn unescape_string_body(body: &str) -> Result<String, String> {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                return Err(
                    "unescaped `\"` inside a string literal; write `\\\"` for a quote".to_owned(),
                );
            }
            '\\' => match chars.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => {
                    return Err(format!(
                        "unknown escape `\\{other}` in a string literal; write `\\\\{other}` for a \
                         regex `\\{other}` (a backslash is written `\\\\`)"
                    ));
                }
                None => return Err("string literal ends in a lone backslash".to_owned()),
            },
            ch => out.push(ch),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::parse_regex_attribute;

    #[test]
    fn doubled_backslash_is_one_backslash() {
        assert_eq!(
            parse_regex_attribute(r#"@regex("^\\+[0-9]{6,15}$")"#).unwrap(),
            r"^\+[0-9]{6,15}$"
        );
    }

    #[test]
    fn plain_patterns_and_escaped_quotes() {
        assert_eq!(
            parse_regex_attribute(r#"@regex("^[a-z]+$")"#).unwrap(),
            "^[a-z]+$"
        );
        assert_eq!(parse_regex_attribute(r#"@regex("a\"b")"#).unwrap(), "a\"b");
        assert_eq!(
            parse_regex_attribute(r#"@regex("a\\\\b")"#).unwrap(),
            r"a\\b"
        );
    }

    #[test]
    fn single_backslash_class_is_rejected_with_the_fix() {
        let err = parse_regex_attribute(r#"@regex("^\d+$")"#).unwrap_err();
        assert!(
            err.contains("unknown escape `\\d`") && err.contains("`\\\\d`"),
            "{err}"
        );
        assert!(parse_regex_attribute(r#"@regex("a\")"#).is_err());
        assert!(parse_regex_attribute(r#"@regex("a"b")"#).is_err());
    }
}
