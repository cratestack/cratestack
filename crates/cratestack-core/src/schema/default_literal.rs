//! What a literal `@default(...)` means for its field (cratestack#1234).
//!
//! A literal default reaches two places. The migration writes it into the
//! column's `DEFAULT` clause, and the embedded runtime, whose tables are
//! not built from migrations, binds it on create. Both need the same
//! answer to "what value is this", so both go through [`literal_default`],
//! which also lets `cratestack check` refuse a literal that does not fit
//! its field instead of letting it fail when the migration is applied.
//!
//! The accepted spellings, by field type:
//!
//! - `Boolean`: `true` or `false`, in any case.
//! - `Int`, `BigInt`: a decimal integer inside `i64`.
//! - `Float`: a finite decimal number, optionally with an exponent.
//! - `Decimal`: a plain decimal number (`-12.50`), bare or quoted.
//! - `String`, `Cuid`, `Uuid`, `DateTime`: a quoted string, `"…"` or `'…'`,
//!   where a backslash escapes the next character (the attribute scanner's
//!   rule, [`super::attribute_text`]). A `Cuid` must be cuid-shaped, a
//!   `Uuid` a UUID, and a `DateTime` an RFC 3339 timestamp.
//! - an enum: one of its variants, bare or quoted.
//!
//! Every other field type takes no literal default.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::model::{EnumDecl, TypeArity, TypeRef};

#[path = "default_literal_scalars.rs"]
mod scalars;

use scalars::{
    bare_word_error, bool_literal, decimal_literal, float_literal, integer_literal, text_literal,
};

/// The value a literal default stands for.
#[derive(Debug, Clone, PartialEq)]
pub enum LiteralValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    /// The number as written, without a leading `+`.
    Decimal(String),
    /// A `String` or `Cuid` value.
    Text(String),
    Uuid(Uuid),
    DateTime(DateTime<Utc>),
    /// An enum variant, with its number when the enum is ordinal.
    Enum {
        variant: String,
        ordinal: Option<i64>,
    },
}

/// A literal default: its value, and the SQL literal a migration writes.
#[derive(Debug, Clone, PartialEq)]
pub struct LiteralDefault {
    pub value: LiteralValue,
    /// The `DEFAULT` text, the same on Postgres and SQLite. A string is
    /// single-quoted, so `"draft"` never reaches SQL as an identifier.
    pub sql: String,
}

/// Interpret `inner`, the text of `@default(inner)`, as a literal for a
/// field of type `ty`. `enum_decl` is the field's enum, when it has one.
/// The error says why the literal does not fit.
pub fn literal_default(
    inner: &str,
    ty: &TypeRef,
    enum_decl: Option<&EnumDecl>,
) -> Result<LiteralDefault, String> {
    let inner = inner.trim();
    if matches!(ty.arity, TypeArity::List) {
        return Err(format!(
            "a list field (`{}[]`) takes no literal default",
            ty.name
        ));
    }
    if inner.eq_ignore_ascii_case("null") {
        return Err(
            "`null` is not a default: an optional field is already NULL when the create input \
             leaves it out, so remove the `@default`"
                .to_owned(),
        );
    }
    if let Some(decl) = enum_decl {
        return enum_literal(inner, decl);
    }
    let unquoted = unquote(inner);
    match ty.name.as_str() {
        "Boolean" => bool_literal(inner),
        "Int" | "BigInt" => integer_literal(inner, &ty.name),
        "Float" => float_literal(inner),
        "Decimal" => decimal_literal(inner, unquoted.as_deref()),
        "String" | "Cuid" | "Uuid" | "DateTime" => match unquoted {
            Some(text) => text_literal(text, &ty.name),
            None => Err(bare_word_error(inner, &ty.name)),
        },
        other => Err(format!("a `{other}` field takes no literal default")),
    }
}

/// The content of a `"…"` or `'…'` literal, unescaped; `None` when `inner`
/// is not one quoted string.
pub fn unquote(inner: &str) -> Option<String> {
    let quote = inner.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let body = inner[1..].strip_suffix(quote)?;
    let mut text = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => text.push(chars.next()?),
            ch if ch == quote => return None,
            ch => text.push(ch),
        }
    }
    Some(text)
}

/// A single-quoted SQL string literal.
pub fn sql_string(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn enum_literal(inner: &str, decl: &EnumDecl) -> Result<LiteralDefault, String> {
    let name = unquote(inner).unwrap_or_else(|| inner.to_owned());
    if !decl.variants.iter().any(|variant| variant.name == name) {
        let variants: Vec<&str> = decl.variants.iter().map(|v| v.name.as_str()).collect();
        return Err(format!(
            "`{name}` is not a variant of `{}` (variants: {})",
            decl.name,
            variants.join(", ")
        ));
    }
    let ordinal = decl.ordinal_of(&name);
    let sql = match ordinal {
        Some(ordinal) => ordinal.to_string(),
        None => sql_string(&name),
    };
    Ok(LiteralDefault {
        value: LiteralValue::Enum {
            variant: name,
            ordinal,
        },
        sql,
    })
}

#[cfg(test)]
#[path = "default_literal_tests.rs"]
mod tests;
