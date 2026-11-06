//! The pure half of the `CompactKeys` binding (compact wire keys,
//! `docs/design/compact-wire-keys.md`): everything but the `wasm-bindgen`
//! plumbing, so it runs under a host `cargo test`. The class in
//! `compact.rs` is a shell around [`Keys`].
//!
//! The tables a client registers are its own current contracts, so each op
//! has one entry and that entry is the one every call uses.
#![cfg_attr(not(test), allow(dead_code))]

use cratestack_codec_cbor::compact::{cbor_from_compact, cbor_to_compact};
use cratestack_codec_json::compact::{json_from_compact, json_to_compact};
use cratestack_core::ContractSelector;
use cratestack_core::compact::{Direction, KeyTables, OpKeys};

/// A client's key tables, loaded from the generated artifact.
pub(crate) struct Keys {
    tables: KeyTables,
}

impl Keys {
    pub(crate) fn from_tables(bytes: &[u8]) -> Result<Keys, String> {
        KeyTables::from_bytes(bytes)
            .map(|tables| Keys { tables })
            .map_err(|error| error.to_string())
    }

    pub(crate) fn to_compact(
        &self,
        named: &[u8],
        op: &str,
        direction: &str,
    ) -> Result<Vec<u8>, String> {
        let (keys, dir) = self.resolve(op, direction)?;
        cbor_to_compact(&self.tables, self.tables.node(keys, dir), named)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn to_named(
        &self,
        compact: &[u8],
        op: &str,
        direction: &str,
    ) -> Result<Vec<u8>, String> {
        let (keys, dir) = self.resolve(op, direction)?;
        cbor_from_compact(&self.tables, self.tables.node(keys, dir), compact)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn to_compact_json(
        &self,
        named: &str,
        op: &str,
        direction: &str,
    ) -> Result<String, String> {
        let (keys, dir) = self.resolve(op, direction)?;
        let out = json_to_compact(&self.tables, self.tables.node(keys, dir), named.as_bytes())
            .map_err(|error| error.to_string())?;
        String::from_utf8(out).map_err(|error| error.to_string())
    }

    pub(crate) fn to_named_json(
        &self,
        compact: &str,
        op: &str,
        direction: &str,
    ) -> Result<String, String> {
        let (keys, dir) = self.resolve(op, direction)?;
        let out = json_from_compact(
            &self.tables,
            self.tables.node(keys, dir),
            compact.as_bytes(),
        )
        .map_err(|error| error.to_string())?;
        String::from_utf8(out).map_err(|error| error.to_string())
    }

    /// The `Cratestack-Contract` header value of `op`'s current contract.
    pub(crate) fn selector(&self, op: &str) -> Option<String> {
        self.entry(op)
            .map(|keys| ContractSelector::of(&keys.digest).to_header_value())
    }

    fn entry(&self, op: &str) -> Option<&OpKeys> {
        self.tables.ops.iter().find(|keys| keys.op == op)
    }

    fn resolve(&self, op: &str, direction: &str) -> Result<(&OpKeys, Direction), String> {
        let dir = match direction {
            "request" => Direction::Request,
            "response" => Direction::Response,
            other => {
                return Err(format!(
                    "direction must be \"request\" or \"response\", got \"{other}\""
                ));
            }
        };
        let keys = self
            .entry(op)
            .ok_or_else(|| format!("no compact key table for op \"{op}\""))?;
        Ok((keys, dir))
    }
}

#[cfg(test)]
mod tests {
    use cratestack_core::compact::{KeyNode, KeyStruct};

    use super::*;

    /// One op, `shop.put`, whose request is `{amount: 0, id: 1}` and whose
    /// response is opaque.
    fn keys() -> Keys {
        let tables = KeyTables {
            types: vec![KeyStruct {
                fields: vec![
                    ("amount".to_owned(), KeyNode::Opaque),
                    ("id".to_owned(), KeyNode::Opaque),
                ],
            }],
            ops: vec![OpKeys {
                op: "shop.put".to_owned(),
                digest: [7; 32],
                request: KeyNode::Struct(0),
                response: KeyNode::Opaque,
            }],
        };
        Keys::from_tables(&tables.to_bytes()).expect("artifact loads")
    }

    // {"id":"a","amount":5} and its compact form {1:"a",0:5}.
    const NAMED: [u8; 14] = [
        0xa2, 0x62, 0x69, 0x64, 0x61, 0x61, 0x66, 0x61, 0x6d, 0x6f, 0x75, 0x6e, 0x74, 0x05,
    ];
    const COMPACT: [u8; 6] = [0xa2, 0x01, 0x61, 0x61, 0x00, 0x05];

    #[test]
    fn cbor_round_trips_through_the_single_entry_of_the_op() {
        let keys = keys();
        assert_eq!(
            keys.to_compact(&NAMED, "shop.put", "request").unwrap(),
            COMPACT
        );
        assert_eq!(
            keys.to_named(&COMPACT, "shop.put", "request").unwrap(),
            NAMED
        );
        // The response tree is opaque: bytes pass through.
        assert_eq!(
            keys.to_compact(&NAMED, "shop.put", "response").unwrap(),
            NAMED
        );
    }

    #[test]
    fn json_round_trips_with_decimal_string_keys() {
        let keys = keys();
        let named = r#"{"id":"a","amount":5}"#;
        let compact = r#"{"1":"a","0":5}"#;
        assert_eq!(
            keys.to_compact_json(named, "shop.put", "request").unwrap(),
            compact
        );
        assert_eq!(
            keys.to_named_json(compact, "shop.put", "request").unwrap(),
            named
        );
    }

    #[test]
    fn selector_is_the_eight_byte_digest_prefix_in_base64url() {
        // 0x07 * 8, unpadded base64url.
        assert_eq!(keys().selector("shop.put").as_deref(), Some("BwcHBwcHBwc"));
        assert_eq!(keys().selector("shop.none"), None);
    }

    #[test]
    fn bad_input_is_an_error_never_a_panic() {
        let keys = keys();
        assert!(keys.to_compact(&NAMED, "shop.none", "request").is_err());
        assert!(keys.to_compact(&NAMED, "shop.put", "sideways").is_err());
        assert!(keys.to_compact(&[0x1b], "shop.put", "request").is_err());
        assert!(keys.to_named_json("{", "shop.put", "request").is_err());
        assert!(Keys::from_tables(&[1, 2, 3]).is_err());
        // An integer key past the table is refused on the way back.
        assert!(
            keys.to_named(&[0xa1, 0x09, 0x00], "shop.put", "request")
                .is_err()
        );
    }
}
