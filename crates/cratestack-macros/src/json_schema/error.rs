//! Typed failures of the `.cstack` → JSON Schema generator.
//!
//! Every failure names the `.cstack` type that caused it, and where that
//! type was reached from, because phase 3 (cratestack#1033) turns these
//! into compile errors on an `@mcp(tool)` procedure. "Your tool can't be
//! exposed because of `Json` in `Holder.payload`" is actionable; "schema
//! generation failed" isn't.
//!
//! There is deliberately no variant that degrades to a permissive `{}`
//! schema. A schema that accepts anything gives an agent no signal and
//! hides the one fact it needs: the server will reject most of what it
//! sends (ADR 0002 § Tools; cratestack#1037's acceptance criteria).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JsonSchemaError {
    /// The type reached procedure I/O, but no JSON Schema describes its
    /// serde wire shape both faithfully and assertively.
    NoFaithfulMapping {
        type_name: String,
        reason: &'static str,
        at: Vec<String>,
    },
    /// A name that is neither a built-in scalar nor a `type`, `enum` or
    /// `model` of this schema. The parser rejects unknown names, so this
    /// only fires for a declaration kind the generator doesn't cover.
    UnknownType { type_name: String, at: Vec<String> },
    /// A `Decimal` was reached without a `decimal = ...` backend. The two
    /// backends serialize differently (see `scalar.rs`), so guessing one
    /// would produce a schema that is wrong for the other.
    MissingDecimalBackend { at: Vec<String> },
}

impl JsonSchemaError {
    /// Prepends one step of "where was this reached from" context. Called
    /// on the way back up the recursion, so the outermost step ends up
    /// first.
    pub(crate) fn within(mut self, step: impl Into<String>) -> Self {
        let at = match &mut self {
            Self::NoFaithfulMapping { at, .. }
            | Self::UnknownType { at, .. }
            | Self::MissingDecimalBackend { at } => at,
        };
        at.insert(0, step.into());
        self
    }
}

impl fmt::Display for JsonSchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (at, message) = match self {
            Self::NoFaithfulMapping {
                type_name,
                reason,
                at,
            } => (
                at,
                format!("`{type_name}` has no faithful JSON Schema mapping: {reason}"),
            ),
            Self::UnknownType { type_name, at } => (
                at,
                format!("`{type_name}` is not a type the JSON Schema generator can resolve"),
            ),
            Self::MissingDecimalBackend { at } => (
                at,
                "`Decimal` needs a `decimal = RustDecimal | BigDecimal` argument to pick its \
                 JSON Schema, because the two backends serialize differently"
                    .to_owned(),
            ),
        };
        if at.is_empty() {
            formatter.write_str(&message)
        } else {
            write!(formatter, "{message} (at {})", at.join(" → "))
        }
    }
}

impl std::error::Error for JsonSchemaError {}
