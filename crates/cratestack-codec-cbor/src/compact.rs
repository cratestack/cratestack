//! Compact wire keys for CBOR (`docs/design/compact-wire-keys.md`): a
//! token-level rewrite of a body's map keys, guided by a type tree.
//!
//! Both directions work on bytes. Encoding runs the normal codec first and
//! then [`cbor_to_compact`] swaps each schema-declared field name for its
//! integer key; decoding runs [`cbor_from_compact`] and then the normal
//! codec. Values the tree does not describe are copied byte for byte, so
//! `Uuid`, `Decimal`, tags and every other representation reach the wire
//! exactly as the codec wrote them.
//!
//! Rules, per [`KeyNode`]:
//!
//! - `Opaque`, or a value that is not the container the node expects:
//!   copied unchanged.
//! - `Struct`: to compact, a text key found in the struct becomes its index
//!   and a text key the struct lacks is **dropped** with its value (the
//!   peer's table predates it); from compact, an integer key becomes its
//!   field name and one past the table is an **error**, while a text key
//!   passes through (mixed maps are accepted).
//! - `List`: every element follows the inner node.
//! - `Page`: the wrapper's keys stay named and only `items` follows
//!   `List(inner)`.
//!
//! Definite and indefinite maps and arrays both work, and tags are
//! transparent.

use cratestack_core::CratestackError;
use cratestack_core::compact::{KeyNode, KeyTables};
use minicbor::Decoder;

mod map;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_shapes;
#[cfg(test)]
mod tests_support;

/// Deepest container nesting a rewrite follows before refusing the body.
const MAX_DEPTH: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    ToCompact,
    FromCompact,
}

/// Rewrites a named CBOR body's field names to integer keys.
///
/// `named` must be exactly one CBOR item.
pub fn cbor_to_compact(
    tables: &KeyTables,
    node: &KeyNode,
    named: &[u8],
) -> Result<Vec<u8>, CratestackError> {
    rewrite(tables, node, named, Mode::ToCompact)
}

/// Rewrites a compact CBOR body's integer keys back to field names.
///
/// `compact` must be exactly one CBOR item.
pub fn cbor_from_compact(
    tables: &KeyTables,
    node: &KeyNode,
    compact: &[u8],
) -> Result<Vec<u8>, CratestackError> {
    rewrite(tables, node, compact, Mode::FromCompact)
}

fn rewrite(
    tables: &KeyTables,
    node: &KeyNode,
    input: &[u8],
    mode: Mode,
) -> Result<Vec<u8>, CratestackError> {
    let mut walk = Walk {
        tables,
        mode,
        input,
    };
    let mut decoder = Decoder::new(input);
    let mut out = Vec::with_capacity(input.len());
    walk.value(&mut decoder, node, &mut out, 0)?;
    if decoder.position() != input.len() {
        return Err(fail("bytes left unconsumed after the value"));
    }
    Ok(out)
}

fn fail(why: impl std::fmt::Display) -> CratestackError {
    CratestackError::Codec(format!("failed to rewrite CBOR body: {why}"))
}

struct Walk<'a> {
    tables: &'a KeyTables,
    mode: Mode,
    input: &'a [u8],
}

impl Walk<'_> {
    /// Rewrites the item at the decoder's position into `out`.
    fn value(
        &mut self,
        d: &mut Decoder<'_>,
        node: &KeyNode,
        out: &mut Vec<u8>,
        depth: usize,
    ) -> Result<(), CratestackError> {
        if depth > MAX_DEPTH {
            return Err(fail("nested too deeply"));
        }
        if matches!(node, KeyNode::Opaque) {
            return self.copy(d, out);
        }
        match d.datatype().map_err(fail)? {
            minicbor::data::Type::Tag => {
                let tag = d.tag().map_err(fail)?;
                minicbor::Encoder::new(&mut *out).tag(tag).map_err(fail)?;
                self.value(d, node, out, depth)
            }
            minicbor::data::Type::Map | minicbor::data::Type::MapIndef => {
                self.map(d, node, out, depth)
            }
            minicbor::data::Type::Array | minicbor::data::Type::ArrayIndef => match node {
                KeyNode::List(inner) => self.list(d, inner, out, depth),
                _ => self.copy(d, out),
            },
            _ => self.copy(d, out),
        }
    }

    /// Copies the item at the decoder's position verbatim.
    fn copy(&self, d: &mut Decoder<'_>, out: &mut Vec<u8>) -> Result<(), CratestackError> {
        let start = d.position();
        d.skip().map_err(fail)?;
        out.extend_from_slice(&self.input[start..d.position()]);
        Ok(())
    }

    fn list(
        &mut self,
        d: &mut Decoder<'_>,
        inner: &KeyNode,
        out: &mut Vec<u8>,
        depth: usize,
    ) -> Result<(), CratestackError> {
        let len = d.array().map_err(fail)?;
        let mut head = minicbor::Encoder::new(&mut *out);
        match len {
            Some(n) => head.array(n).map(drop),
            None => head.begin_array().map(drop),
        }
        .map_err(fail)?;
        let mut seen = 0;
        while len.is_none_or(|n| seen < n) {
            if len.is_none() && at_break(d)? {
                break;
            }
            self.value(d, inner, out, depth + 1)?;
            seen += 1;
        }
        if len.is_none() {
            out.push(0xff);
        }
        Ok(())
    }
}

/// Whether the decoder is at an indefinite container's break; consumes it.
fn at_break(d: &mut Decoder<'_>) -> Result<bool, CratestackError> {
    if d.datatype().map_err(fail)? == minicbor::data::Type::Break {
        d.set_position(d.position() + 1);
        return Ok(true);
    }
    Ok(false)
}
