//! Explicit enum ordinals (`Pending = 1`, `@@reserved(4)`), the IR half of
//! `docs/design/enum-ordinals.md`. The parser fills these in and checks
//! rules R1–R4; nothing generates from them yet, which is why validation
//! refuses an ordinal enum until a backend does.

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
