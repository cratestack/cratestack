//! The pure half of the `CompactKeys` binding (compact wire keys,
//! `docs/design/compact-wire-keys.md`): everything but the napi plumbing,
//! so it runs under a host `cargo test` (see the module docs of `lib.rs`
//! for why that split exists). The class in `compact.rs` is a shell around
//! [`Keys`].
//!
//! The tables a client registers are its own current contracts, so each op
//! has one entry and that entry is the one every call uses.

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
mod tests;
