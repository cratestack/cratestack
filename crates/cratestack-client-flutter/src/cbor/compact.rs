//! Compact wire keys over the bridge (`docs/design/compact-wire-keys.md`):
//! [`FlutterCompactKeys`] swaps a client's schema field names for integer
//! keys on the wire and back.
//!
//! Opaque across the bridge, like the COSE envelope: the tables stay in
//! Rust and Dart passes bytes. `direction` is the string `"request"` or
//! `"response"`, not an enum, because an enum would bring a generated
//! `freezed_annotation` import the package does not declare (CI's glue gate
//! forbids it) and because the generated Dart hooks take strings.
//!
//! No wire logic lives here: the rewrite is `cratestack-codec-cbor`'s and
//! `cratestack-codec-json`'s, and the tables are `cratestack-core`'s. The
//! tables a client registers are its own current contracts, so each op has
//! one entry and every call uses it. Not behind the `cose` feature.

use cratestack_client_rust::RuntimeErrorCode;
use cratestack_codec_cbor::compact::{cbor_from_compact, cbor_to_compact};
use cratestack_codec_json::compact::{json_from_compact, json_to_compact};
use cratestack_core::ContractSelector;
use cratestack_core::compact::{Direction, KeyTables, OpKeys};

use super::to_flutter_error;
use crate::types::FlutterRuntimeError;

#[cfg(test)]
mod tests;

/// A client's key tables, loaded from the generated `cratestackKeyTables`.
#[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(opaque))]
pub struct FlutterCompactKeys {
    tables: KeyTables,
}

impl FlutterCompactKeys {
    /// Loads the key-table artifact.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync))]
    pub fn from_tables(tables: Vec<u8>) -> Result<Self, FlutterRuntimeError> {
        KeyTables::from_bytes(&tables)
            .map(|tables| Self { tables })
            .map_err(|error| to_flutter_error(RuntimeErrorCode::BadInput, error))
    }

    /// Named CBOR bytes to compact CBOR bytes.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync))]
    pub fn to_compact(
        &self,
        named: Vec<u8>,
        op: String,
        direction: String,
    ) -> Result<Vec<u8>, FlutterRuntimeError> {
        let (keys, dir) = self.resolve(&op, &direction)?;
        cbor_to_compact(&self.tables, self.tables.node(keys, dir), &named)
            .map_err(|error| to_flutter_error(RuntimeErrorCode::Codec, error))
    }

    /// Compact CBOR bytes back to named CBOR bytes.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync))]
    pub fn to_named(
        &self,
        compact: Vec<u8>,
        op: String,
        direction: String,
    ) -> Result<Vec<u8>, FlutterRuntimeError> {
        let (keys, dir) = self.resolve(&op, &direction)?;
        cbor_from_compact(&self.tables, self.tables.node(keys, dir), &compact)
            .map_err(|error| to_flutter_error(RuntimeErrorCode::Codec, error))
    }

    /// Named JSON text to compact JSON text (decimal-string keys).
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync))]
    pub fn to_compact_json(
        &self,
        named: String,
        op: String,
        direction: String,
    ) -> Result<String, FlutterRuntimeError> {
        let (keys, dir) = self.resolve(&op, &direction)?;
        let out = json_to_compact(&self.tables, self.tables.node(keys, dir), named.as_bytes())
            .map_err(|error| to_flutter_error(RuntimeErrorCode::Codec, error))?;
        utf8(out)
    }

    /// Compact JSON text back to named JSON text.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync))]
    pub fn to_named_json(
        &self,
        compact: String,
        op: String,
        direction: String,
    ) -> Result<String, FlutterRuntimeError> {
        let (keys, dir) = self.resolve(&op, &direction)?;
        let out = json_from_compact(
            &self.tables,
            self.tables.node(keys, dir),
            compact.as_bytes(),
        )
        .map_err(|error| to_flutter_error(RuntimeErrorCode::Codec, error))?;
        utf8(out)
    }

    /// The `Cratestack-Contract` header value of `op`'s current contract
    /// (8 digest bytes, unpadded base64url), or `None` for an op the tables
    /// lack.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync))]
    pub fn selector(&self, op: String) -> Option<String> {
        self.entry(&op)
            .map(|keys| ContractSelector::of(&keys.digest).to_header_value())
    }

    fn entry(&self, op: &str) -> Option<&OpKeys> {
        self.tables.ops.iter().find(|keys| keys.op == op)
    }

    fn resolve(
        &self,
        op: &str,
        direction: &str,
    ) -> Result<(&OpKeys, Direction), FlutterRuntimeError> {
        let dir = match direction {
            "request" => Direction::Request,
            "response" => Direction::Response,
            other => {
                return Err(to_flutter_error(
                    RuntimeErrorCode::BadInput,
                    format!("direction must be \"request\" or \"response\", got \"{other}\""),
                ));
            }
        };
        let keys = self.entry(op).ok_or_else(|| {
            to_flutter_error(
                RuntimeErrorCode::BadInput,
                format!("no compact key table for op \"{op}\""),
            )
        })?;
        Ok((keys, dir))
    }
}

fn utf8(bytes: Vec<u8>) -> Result<String, FlutterRuntimeError> {
    String::from_utf8(bytes).map_err(|error| to_flutter_error(RuntimeErrorCode::Codec, error))
}
