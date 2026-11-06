//! The `CompactKeys` class: compact wire keys for Node
//! (`docs/design/compact-wire-keys.md`). A thin napi shell over
//! `compact_keys::Keys`, which holds the logic and is tested by `cargo
//! test`; an error is a catchable JS `Error`, never a crash.

use napi::bindgen_prelude::Uint8Array;
use napi_derive::napi;

use crate::compact_keys::Keys;

fn fail(message: String) -> napi::Error {
    napi::Error::new(napi::Status::GenericFailure, message)
}

/// The key tables of one client: swaps field names for integer keys on
/// the wire and back. `direction` is `"request"` or `"response"`.
#[napi]
pub struct CompactKeys {
    inner: Keys,
}

#[napi]
impl CompactKeys {
    /// Loads the generated key-table artifact.
    #[napi(factory, catch_unwind)]
    pub fn from_tables(tables: Uint8Array) -> napi::Result<Self> {
        Keys::from_tables(tables.as_ref())
            .map(|inner| CompactKeys { inner })
            .map_err(fail)
    }

    /// Named CBOR bytes to compact CBOR bytes.
    #[napi(catch_unwind)]
    pub fn to_compact(
        &self,
        named: Uint8Array,
        op: String,
        direction: String,
    ) -> napi::Result<Uint8Array> {
        self.inner
            .to_compact(named.as_ref(), &op, &direction)
            .map(Uint8Array::from)
            .map_err(fail)
    }

    /// Compact CBOR bytes back to named CBOR bytes.
    #[napi(js_name = "fromCompact", catch_unwind)]
    pub fn named(
        &self,
        compact: Uint8Array,
        op: String,
        direction: String,
    ) -> napi::Result<Uint8Array> {
        self.inner
            .to_named(compact.as_ref(), &op, &direction)
            .map(Uint8Array::from)
            .map_err(fail)
    }

    /// Named JSON text to compact JSON text (decimal-string keys).
    #[napi(catch_unwind)]
    pub fn to_compact_json(
        &self,
        named: String,
        op: String,
        direction: String,
    ) -> napi::Result<String> {
        self.inner
            .to_compact_json(&named, &op, &direction)
            .map_err(fail)
    }

    /// Compact JSON text back to named JSON text.
    #[napi(js_name = "fromCompactJson", catch_unwind)]
    pub fn named_json(
        &self,
        compact: String,
        op: String,
        direction: String,
    ) -> napi::Result<String> {
        self.inner
            .to_named_json(&compact, &op, &direction)
            .map_err(fail)
    }

    /// The `Cratestack-Contract` header value of `op`'s current contract,
    /// or `undefined` for an op the tables lack.
    #[napi(catch_unwind)]
    pub fn selector(&self, op: String) -> Option<String> {
        self.inner.selector(&op)
    }
}
