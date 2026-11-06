//! Explicit enum ordinals (`Pending = 1`, `@@reserved(4)`), the IR half of
//! `docs/design/enum-ordinals.md`. The parser fills these in and checks
//! rules R1–R5; the macros, `cratestack-migrate` and the client generators
//! read them back through [`EnumDecl::is_ordinal`](super::model::EnumDecl::is_ordinal).

use serde::{Deserialize, Serialize};

use super::SourceSpan;

/// The largest ordinal a variant may carry: the column is a `SMALLINT`.
pub const ENUM_ORDINAL_MAX: i64 = i16::MAX as i64;

/// The block attribute that retires ordinals, so a removed variant's
/// number can never be handed to a new one.
pub const ENUM_RESERVED_ATTRIBUTE: &str = "@@reserved";

/// An enum's `@@reserved(n, …)` line: ordinals no variant may use, ever.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReservedOrdinals {
    pub values: Vec<i64>,
    pub span: SourceSpan,
}

impl super::model::EnumDecl {
    /// Whether the enum is stored and sent as its ordinals. Validation
    /// guarantees all variants are numbered or none is.
    pub fn is_ordinal(&self) -> bool {
        self.variants.iter().any(|v| v.ordinal.is_some())
    }

    /// The ordinal `variant` carries, if the enum is ordinal and has it.
    pub fn ordinal_of(&self, variant: &str) -> Option<i64> {
        self.variants
            .iter()
            .find(|v| v.name == variant)
            .and_then(|v| v.ordinal)
    }
}
