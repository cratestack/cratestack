//! The `CompactKeys` class: compact wire keys for JavaScript
//! (`docs/design/compact-wire-keys.md`). A thin shell over
//! `compact_keys::Keys`; it rewrites bytes and never panics, so a bad
//! input is a catchable `Error`, not a trap. Not behind the `cose`
//! feature: the codec-only npm build carries it.

use wasm_bindgen::prelude::*;

use crate::compact_keys::Keys;

fn fail(message: String) -> JsError {
    JsError::new(&message)
}

/// The key tables of one client: swaps field names for integer keys on
/// the wire and back. Build it with `createCompactKeys` in the JS wrapper.
#[wasm_bindgen]
pub struct CompactKeys {
    inner: Keys,
}

#[wasm_bindgen]
impl CompactKeys {
    /// Loads the generated key-table artifact.
    #[wasm_bindgen(js_name = fromTables)]
    pub fn from_tables(tables: &[u8]) -> Result<CompactKeys, JsError> {
        Keys::from_tables(tables)
            .map(|inner| CompactKeys { inner })
            .map_err(fail)
    }

    /// Named CBOR bytes to compact CBOR bytes. `direction` is
    /// `"request"` or `"response"`.
    #[wasm_bindgen(js_name = toCompact)]
    pub fn to_compact(&self, named: &[u8], op: &str, direction: &str) -> Result<Vec<u8>, JsError> {
        self.inner.to_compact(named, op, direction).map_err(fail)
    }

    /// Compact CBOR bytes back to named CBOR bytes.
    #[wasm_bindgen(js_name = fromCompact)]
    pub fn named(&self, compact: &[u8], op: &str, direction: &str) -> Result<Vec<u8>, JsError> {
        self.inner.to_named(compact, op, direction).map_err(fail)
    }

    /// Named JSON text to compact JSON text (decimal-string keys).
    #[wasm_bindgen(js_name = toCompactJson)]
    pub fn to_compact_json(
        &self,
        named: &str,
        op: &str,
        direction: &str,
    ) -> Result<String, JsError> {
        self.inner
            .to_compact_json(named, op, direction)
            .map_err(fail)
    }

    /// Compact JSON text back to named JSON text.
    #[wasm_bindgen(js_name = fromCompactJson)]
    pub fn named_json(&self, compact: &str, op: &str, direction: &str) -> Result<String, JsError> {
        self.inner
            .to_named_json(compact, op, direction)
            .map_err(fail)
    }

    /// The `Cratestack-Contract` header value of `op`'s current contract,
    /// or `undefined` for an op the tables lack.
    pub fn selector(&self, op: &str) -> Option<String> {
        self.inner.selector(op)
    }
}
