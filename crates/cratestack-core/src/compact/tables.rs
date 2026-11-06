//! The key-table data model and its lookups.

use crate::ContractSelector;

/// Which side of an op a node describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Request,
    Response,
}

/// A type tree node: how the keys of one value are numbered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyNode {
    /// Copied as is: scalars, enums, `Json`, framework-owned frames, and
    /// anything the contract does not describe.
    Opaque,
    /// A map whose keys are the fields of `KeyTables::types[index]`.
    Struct(u32),
    /// An array whose elements all follow the inner node.
    List(Box<KeyNode>),
    /// `cratestack_core::Page<T>`: the wrapper's keys (`items`,
    /// `totalCount`, `pageInfo`) stay named and only the `items` array is
    /// mapped, element by element, with the inner node.
    Page(Box<KeyNode>),
}

/// One struct: its fields in key order, so field *n* has key *n*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyStruct {
    pub fields: Vec<(String, KeyNode)>,
}

/// One op at one contract digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpKeys {
    /// The op key: the RPC op id, or `"<METHOD> <route>"` on REST.
    pub op: String,
    /// The op contract digest ([`crate::op_contract_digest`]).
    pub digest: [u8; 32],
    pub request: KeyNode,
    pub response: KeyNode,
}

/// The key tables of a schema (or of a server's accepted contracts).
///
/// `ops` is kept sorted by `(op, digest)`, so two tables built from the same
/// contracts are equal and serialise to the same bytes whatever the order
/// the contracts were added in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyTables {
    pub types: Vec<KeyStruct>,
    pub ops: Vec<OpKeys>,
}

impl KeyTables {
    /// The op `op` whose digest starts with `selector`'s bytes. When two
    /// accepted digests share the prefix, the first in digest order wins.
    pub fn find(&self, op: &str, selector: &ContractSelector) -> Option<&OpKeys> {
        self.of_op(op)
            .iter()
            .find(|keys| selector.matches(&keys.digest))
    }

    /// The op `op` at exactly `digest`.
    pub fn find_digest(&self, op: &str, digest: &[u8; 32]) -> Option<&OpKeys> {
        self.of_op(op).iter().find(|keys| &keys.digest == digest)
    }

    /// The root node of `keys` for one direction.
    pub fn node<'a>(&self, keys: &'a OpKeys, dir: Direction) -> &'a KeyNode {
        match dir {
            Direction::Request => &keys.request,
            Direction::Response => &keys.response,
        }
    }

    /// The contiguous run of `ops` entries for `op`.
    fn of_op(&self, op: &str) -> &[OpKeys] {
        let start = self.ops.partition_point(|keys| keys.op.as_str() < op);
        let len = self.ops[start..]
            .iter()
            .take_while(|keys| keys.op == op)
            .count();
        &self.ops[start..start + len]
    }

    /// Inserts `keys`, replacing an entry with the same `(op, digest)`.
    pub(super) fn insert_op(&mut self, keys: OpKeys) {
        let probe =
            |other: &OpKeys| (other.op.as_str(), other.digest) < (keys.op.as_str(), keys.digest);
        let at = self.ops.partition_point(probe);
        match self.ops.get(at) {
            Some(same) if same.op == keys.op && same.digest == keys.digest => self.ops[at] = keys,
            _ => self.ops.insert(at, keys),
        }
    }
}

impl KeyNode {
    /// `List(inner)`, except that a list of opaque values is opaque.
    pub(super) fn list(inner: KeyNode) -> KeyNode {
        match inner {
            KeyNode::Opaque => KeyNode::Opaque,
            other => KeyNode::List(Box::new(other)),
        }
    }

    /// `Page(inner)`, except that a page of opaque items is opaque.
    pub(super) fn page(inner: KeyNode) -> KeyNode {
        match inner {
            KeyNode::Opaque => KeyNode::Opaque,
            other => KeyNode::Page(Box::new(other)),
        }
    }

    /// Every struct index moved up by `by`.
    pub(super) fn shifted(&self, by: u32) -> KeyNode {
        match self {
            KeyNode::Opaque => KeyNode::Opaque,
            KeyNode::Struct(i) => KeyNode::Struct(i + by),
            KeyNode::List(n) => KeyNode::List(Box::new(n.shifted(by))),
            KeyNode::Page(n) => KeyNode::Page(Box::new(n.shifted(by))),
        }
    }
}
