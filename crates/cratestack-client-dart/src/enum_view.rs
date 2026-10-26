//! The view of one `.cstack` enum the Dart templates render, string or
//! ordinal (`Pending = 1`, `docs/design/enum-ordinals.md`).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EnumView {
    pub(crate) name: String,
    pub(crate) variants: Vec<EnumVariantView>,
    /// Sent as its variants' numbers (`Pending = 1`) rather than names.
    pub(crate) is_ordinal: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EnumVariantView {
    pub(crate) identifier: String,
    pub(crate) wire_name: String,
    pub(crate) ordinal: Option<i64>,
}
