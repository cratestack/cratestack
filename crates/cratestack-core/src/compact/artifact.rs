//! The key-table artifact: what generated clients and servers embed.
//!
//! Deterministic CBOR, positional, no keys of its own:
//!
//! ```text
//! artifact = [ format, structs, ops ]            format = 1
//! structs  = [ struct* ]                         struct = [ (name, node)* ]  (flat: name, node, name, node, ...)
//! ops      = [ op* ]                             op     = [ op key (text), digest (32 bytes), request node, response node ]
//! node     = 0            Opaque
//!          | n            Struct(n - 1)         (n >= 1)
//!          | [0, node]    List
//!          | [1, node]    Page
//! ```
//!
//! `ops` is sorted by `(op, digest)`, which [`KeyTables`] keeps true itself.

use std::convert::Infallible;

use minicbor::decode::Decoder;
use minicbor::encode::{Encoder, Error as EncodeError};

use super::error::CompactError;
use super::tables::{KeyNode, KeyStruct, KeyTables, OpKeys};

/// A reason the artifact is unreadable.
struct Bad(String);

impl From<minicbor::decode::Error> for Bad {
    fn from(error: minicbor::decode::Error) -> Self {
        Bad(error.to_string())
    }
}

fn bad<T>(why: impl Into<String>) -> Result<T, Bad> {
    Err(Bad(why.into()))
}

/// The artifact layout this build writes and reads.
const FORMAT: u32 = 1;
/// Deepest `List`/`Page` nesting a reader accepts.
const MAX_DEPTH: usize = 16;

impl KeyTables {
    /// The artifact bytes: deterministic, so equal tables give equal bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut Encoder::new(&mut out))
            .expect("writing to a Vec cannot fail");
        out
    }

    /// Reads an artifact [`to_bytes`](Self::to_bytes) wrote. Refuses another
    /// format number, trailing bytes, a struct index past the table and a
    /// digest that is not 32 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<KeyTables, CompactError> {
        let mut d = Decoder::new(bytes);
        let tables = read(&mut d).map_err(|Bad(why)| CompactError::Artifact(why))?;
        if d.position() != bytes.len() {
            return Err(CompactError::Artifact("trailing bytes".to_owned()));
        }
        Ok(tables)
    }

    fn write(&self, e: &mut Encoder<&mut Vec<u8>>) -> Result<(), EncodeError<Infallible>> {
        e.array(3)?.u32(FORMAT)?;
        e.array(self.types.len() as u64)?;
        for s in &self.types {
            e.array(2 * s.fields.len() as u64)?;
            for (name, node) in &s.fields {
                e.str(name)?;
                write_node(e, node)?;
            }
        }
        e.array(self.ops.len() as u64)?;
        for op in &self.ops {
            e.array(4)?.str(&op.op)?.bytes(&op.digest)?;
            write_node(e, &op.request)?;
            write_node(e, &op.response)?;
        }
        Ok(())
    }
}

fn write_node(
    e: &mut Encoder<&mut Vec<u8>>,
    node: &KeyNode,
) -> Result<(), EncodeError<Infallible>> {
    match node {
        KeyNode::Opaque => e.u32(0).map(drop),
        KeyNode::Struct(i) => e.u32(i + 1).map(drop),
        KeyNode::List(inner) => write_node(e.array(2)?.u8(0)?, inner),
        KeyNode::Page(inner) => write_node(e.array(2)?.u8(1)?, inner),
    }
}

fn read(d: &mut Decoder<'_>) -> Result<KeyTables, Bad> {
    if d.array()? != Some(3) {
        return bad("expected a 3-element array");
    }
    let format = d.u32()?;
    if format != FORMAT {
        return bad(format!("format {format}, this build reads {FORMAT}"));
    }
    let mut tables = KeyTables::default();
    let count = d.array()?.ok_or_else(|| Bad("indefinite structs".into()))?;
    for _ in 0..count {
        let len = d.array()?.ok_or_else(|| Bad("indefinite struct".into()))?;
        let mut fields = Vec::new();
        for _ in 0..len / 2 {
            fields.push((d.str()?.to_owned(), read_node(d, 0)?));
        }
        tables.types.push(KeyStruct { fields });
    }
    let count = d.array()?.ok_or_else(|| Bad("indefinite ops".into()))?;
    for _ in 0..count {
        if d.array()? != Some(4) {
            return bad("expected a 4-element op");
        }
        let op = d.str()?.to_owned();
        let digest: [u8; 32] = d
            .bytes()?
            .try_into()
            .map_err(|_| Bad("digest is not 32 bytes".into()))?;
        let request = read_node(d, 0)?;
        let response = read_node(d, 0)?;
        tables.ops.push(OpKeys {
            op,
            digest,
            request,
            response,
        });
    }
    check(&tables)?;
    Ok(tables)
}

fn read_node(d: &mut Decoder<'_>, depth: usize) -> Result<KeyNode, Bad> {
    if depth > MAX_DEPTH {
        return bad("nodes nest too deeply");
    }
    if d.datatype()? == minicbor::data::Type::Array {
        if d.array()? != Some(2) {
            return bad("expected a 2-element node");
        }
        let tag = d.u8()?;
        let inner = Box::new(read_node(d, depth + 1)?);
        return match tag {
            0 => Ok(KeyNode::List(inner)),
            1 => Ok(KeyNode::Page(inner)),
            other => bad(format!("unknown node kind {other}")),
        };
    }
    Ok(match d.u32()? {
        0 => KeyNode::Opaque,
        n => KeyNode::Struct(n - 1),
    })
}

/// Every struct index in the tables points at a struct, and the ops are in
/// the order `KeyTables` keeps (so equality with the writer's tables holds).
fn check(tables: &KeyTables) -> Result<(), Bad> {
    let limit = tables.types.len() as u32;
    let nodes = tables
        .types
        .iter()
        .flat_map(|s| s.fields.iter().map(|(_, n)| n))
        .chain(tables.ops.iter().flat_map(|o| [&o.request, &o.response]));
    for node in nodes {
        if !in_range(node, limit) {
            return bad("struct index out of range");
        }
    }
    let sorted = tables
        .ops
        .windows(2)
        .all(|w| (w[0].op.as_str(), w[0].digest) < (w[1].op.as_str(), w[1].digest));
    if !sorted {
        return bad("ops are not sorted and unique");
    }
    Ok(())
}

fn in_range(node: &KeyNode, limit: u32) -> bool {
    match node {
        KeyNode::Opaque => true,
        KeyNode::Struct(i) => *i < limit,
        KeyNode::List(n) | KeyNode::Page(n) => in_range(n, limit),
    }
}
