//! The compact media-type family and its mapping to the plain types.

/// A CBOR body whose schema-declared field names are integer keys.
pub const COMPACT_CBOR: &str = "application/vnd.cratestack.compact+cbor";
/// A JSON body whose schema-declared field names are decimal-string keys.
pub const COMPACT_JSON: &str = "application/vnd.cratestack.compact+json";
/// A CBOR sequence (`@stream`, subscriptions) of compact CBOR items.
pub const COMPACT_CBOR_SEQ: &str = "application/vnd.cratestack.compact+cbor-seq";

/// The compact type of a plain payload type: `application/cbor`,
/// `application/json` or `application/cbor-seq`. Anything else has none.
///
/// ```
/// use cratestack_core::compact::{COMPACT_CBOR, compact_media_type_for};
/// assert_eq!(compact_media_type_for("application/cbor"), Some(COMPACT_CBOR));
/// assert_eq!(compact_media_type_for("text/plain"), None);
/// ```
pub fn compact_media_type_for(plain: &str) -> Option<&'static str> {
    match plain {
        "application/cbor" => Some(COMPACT_CBOR),
        "application/json" => Some(COMPACT_JSON),
        "application/cbor-seq" => Some(COMPACT_CBOR_SEQ),
        _ => None,
    }
}

/// The plain type a compact type stands for; the inverse of
/// [`compact_media_type_for`].
///
/// ```
/// use cratestack_core::compact::{COMPACT_JSON, plain_media_type_for};
/// assert_eq!(plain_media_type_for(COMPACT_JSON), Some("application/json"));
/// assert_eq!(plain_media_type_for("application/json"), None);
/// ```
pub fn plain_media_type_for(compact: &str) -> Option<&'static str> {
    match compact {
        COMPACT_CBOR => Some("application/cbor"),
        COMPACT_JSON => Some("application/json"),
        COMPACT_CBOR_SEQ => Some("application/cbor-seq"),
        _ => None,
    }
}
