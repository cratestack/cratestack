//! The per-type rules of [`super::literal_default`].

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::{LiteralDefault, LiteralValue, sql_string};

pub(super) fn bool_literal(inner: &str) -> Result<LiteralDefault, String> {
    let value = if inner.eq_ignore_ascii_case("true") {
        true
    } else if inner.eq_ignore_ascii_case("false") {
        false
    } else {
        return Err("a `Boolean` default is `true` or `false`".to_owned());
    };
    Ok(LiteralDefault {
        value: LiteralValue::Bool(value),
        sql: value.to_string(),
    })
}

pub(super) fn integer_literal(inner: &str, ty: &str) -> Result<LiteralDefault, String> {
    let digits = inner.strip_prefix(['+', '-']).unwrap_or(inner);
    match inner.parse::<i64>() {
        Ok(value) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
            Ok(LiteralDefault {
                value: LiteralValue::Int(value),
                sql: value.to_string(),
            })
        }
        _ => Err(format!(
            "an `{ty}` default is a decimal integer inside {}..={}",
            i64::MIN,
            i64::MAX
        )),
    }
}

pub(super) fn float_literal(inner: &str) -> Result<LiteralDefault, String> {
    let numeric = !inner.is_empty()
        && inner
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
        && inner.bytes().any(|b| b.is_ascii_digit());
    match inner.parse::<f64>() {
        Ok(value) if numeric && value.is_finite() => Ok(LiteralDefault {
            value: LiteralValue::Float(value),
            sql: inner.trim_start_matches('+').to_owned(),
        }),
        _ => Err("a `Float` default is a finite decimal number".to_owned()),
    }
}

pub(super) fn decimal_literal(
    inner: &str,
    unquoted: Option<&str>,
) -> Result<LiteralDefault, String> {
    let number = unquoted.unwrap_or(inner).trim_start_matches('+');
    let (whole, fraction) = number.split_once('.').unwrap_or((number, "0"));
    let whole = whole.strip_prefix('-').unwrap_or(whole);
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || !digits(fraction) {
        return Err("a `Decimal` default is a plain decimal number, such as `12.50`".to_owned());
    }
    let sql = match unquoted {
        Some(_) => sql_string(number),
        None => number.to_owned(),
    };
    Ok(LiteralDefault {
        value: LiteralValue::Decimal(number.to_owned()),
        sql,
    })
}

pub(super) fn text_literal(text: String, ty: &str) -> Result<LiteralDefault, String> {
    let sql = sql_string(&text);
    let value = match ty {
        "Uuid" => LiteralValue::Uuid(
            Uuid::parse_str(&text).map_err(|_| format!("`{text}` is not a UUID"))?,
        ),
        "DateTime" => LiteralValue::DateTime(
            DateTime::parse_from_rfc3339(&text)
                .map_err(|_| format!("`{text}` is not an RFC 3339 timestamp"))?
                .with_timezone(&Utc),
        ),
        "Cuid" if crate::error::parse_cuid(&text).is_err() => {
            return Err(format!(
                "`{text}` is not a cuid (lowercase letters and digits)"
            ));
        }
        _ => LiteralValue::Text(text),
    };
    Ok(LiteralDefault { value, sql })
}

/// A bare word on a text-like field: an SQL keyword such as
/// `CURRENT_TIMESTAMP`, which only a database can evaluate, or an
/// unquoted string.
pub(super) fn bare_word_error(inner: &str, ty: &str) -> String {
    let keyword = inner.to_ascii_uppercase();
    if matches!(
        keyword.as_str(),
        "CURRENT_TIMESTAMP" | "CURRENT_DATE" | "CURRENT_TIME" | "LOCALTIMESTAMP" | "LOCALTIME"
    ) {
        return format!(
            "`{inner}` is an SQL expression, not a value; for the current time write \
             `@default(now())`"
        );
    }
    format!("a `{ty}` default is a quoted string: write `@default(\"{inner}\")`")
}
