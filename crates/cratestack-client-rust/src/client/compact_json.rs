//! The compact-JSON half of `compact_wire`, behind the `codec-json`
//! feature that owns the `cratestack-codec-json` dependency. Without it
//! the client has no JSON codec, so it never asks for compact JSON; a
//! compact JSON body that still arrives is refused rather than misread.

use cratestack_core::CratestackError;
use cratestack_core::compact::{KeyNode, KeyTables};

#[cfg(feature = "codec-json")]
pub(crate) fn to_compact(
    tables: &KeyTables,
    node: &KeyNode,
    named: &[u8],
) -> Result<Vec<u8>, CratestackError> {
    cratestack_codec_json::compact::json_to_compact(tables, node, named)
}

#[cfg(feature = "codec-json")]
pub(crate) fn from_compact(
    tables: &KeyTables,
    node: &KeyNode,
    compact: &[u8],
) -> Result<Vec<u8>, CratestackError> {
    cratestack_codec_json::compact::json_from_compact(tables, node, compact)
}

#[cfg(not(feature = "codec-json"))]
pub(crate) fn to_compact(
    _tables: &KeyTables,
    _node: &KeyNode,
    _named: &[u8],
) -> Result<Vec<u8>, CratestackError> {
    Err(unavailable())
}

#[cfg(not(feature = "codec-json"))]
pub(crate) fn from_compact(
    _tables: &KeyTables,
    _node: &KeyNode,
    _compact: &[u8],
) -> Result<Vec<u8>, CratestackError> {
    Err(unavailable())
}

#[cfg(not(feature = "codec-json"))]
fn unavailable() -> CratestackError {
    CratestackError::Codec("compact JSON needs the `codec-json` feature".to_owned())
}
