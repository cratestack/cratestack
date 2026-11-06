//! Compact wire keys for JSON (`docs/design/compact-wire-keys.md`): the
//! twin of `cratestack_codec_cbor::compact`, with an integer key spelled as
//! a decimal string (`"3"`).
//!
//! The rewrite scans the bytes and touches only object keys that the type
//! tree describes. Every other value (numbers, strings, `Json` payloads,
//! whole subtrees the tree leaves opaque) is copied byte for byte, so no
//! number is reparsed and no key order changes. Whitespace is kept inside
//! copied spans and dropped between the tokens of a container that is
//! rewritten, and around the top-level value; bodies from `JsonCodec` have
//! none, so for them a round trip is byte-identical.
//!
//! The rules are the CBOR ones: to compact, a key the struct lacks is
//! dropped with its value; from compact, a numeric key past the table is an
//! error, while a name passes through (mixed objects are accepted). A
//! `Page` keeps its wrapper keys named and maps only `items`.

use cratestack_core::CratestackError;
use cratestack_core::compact::{KeyNode, KeyTables};
use serde::de::IgnoredAny;

mod object;
#[cfg(test)]
mod tests;

/// Deepest container nesting a rewrite follows before refusing the body.
const MAX_DEPTH: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    ToCompact,
    FromCompact,
}

/// Rewrites a named JSON body's field names to decimal-string keys.
pub fn json_to_compact(
    tables: &KeyTables,
    node: &KeyNode,
    named: &[u8],
) -> Result<Vec<u8>, CratestackError> {
    rewrite(tables, node, named, Mode::ToCompact)
}

/// Rewrites a compact JSON body's numeric keys back to field names.
pub fn json_from_compact(
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
    let mut scan = Scan {
        tables,
        mode,
        src: input,
        pos: 0,
    };
    let mut out = Vec::with_capacity(input.len());
    scan.value(node, &mut out, 0)?;
    scan.ws();
    if scan.pos != input.len() {
        return Err(fail("characters left after the value"));
    }
    Ok(out)
}

fn fail(why: impl std::fmt::Display) -> CratestackError {
    CratestackError::Codec(format!("failed to rewrite JSON body: {why}"))
}

struct Scan<'a> {
    tables: &'a KeyTables,
    mode: Mode,
    src: &'a [u8],
    pos: usize,
}

impl Scan<'_> {
    fn ws(&mut self) {
        while matches!(self.src.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Result<u8, CratestackError> {
        self.ws();
        self.src
            .get(self.pos)
            .copied()
            .ok_or_else(|| fail("unexpected end of input"))
    }

    fn expect(&mut self, byte: u8) -> Result<(), CratestackError> {
        if self.peek()? != byte {
            return Err(fail(format!("expected `{}`", byte as char)));
        }
        self.pos += 1;
        Ok(())
    }

    /// Copies the value at the cursor verbatim, validating it on the way.
    fn copy(&mut self, out: &mut Vec<u8>) -> Result<(), CratestackError> {
        self.peek()?;
        let mut items =
            serde_json::Deserializer::from_slice(&self.src[self.pos..]).into_iter::<IgnoredAny>();
        items
            .next()
            .ok_or_else(|| fail("unexpected end of input"))?
            .map_err(fail)?;
        let end = self.pos + items.byte_offset();
        out.extend_from_slice(&self.src[self.pos..end]);
        self.pos = end;
        Ok(())
    }

    /// Reads the string literal at the cursor: its unescaped text and span.
    fn string(&mut self) -> Result<(String, std::ops::Range<usize>), CratestackError> {
        self.peek()?;
        let start = self.pos;
        let mut items =
            serde_json::Deserializer::from_slice(&self.src[start..]).into_iter::<String>();
        let text = items
            .next()
            .ok_or_else(|| fail("unexpected end of input"))?
            .map_err(fail)?;
        self.pos = start + items.byte_offset();
        Ok((text, start..self.pos))
    }

    fn value(
        &mut self,
        node: &KeyNode,
        out: &mut Vec<u8>,
        depth: usize,
    ) -> Result<(), CratestackError> {
        if depth > MAX_DEPTH {
            return Err(fail("nested too deeply"));
        }
        match (self.peek()?, node) {
            (b'{', KeyNode::Struct(_) | KeyNode::Page(_)) => self.object(node, out, depth),
            (b'[', KeyNode::List(inner)) => self.array(inner, out, depth),
            _ => self.copy(out),
        }
    }

    fn array(
        &mut self,
        inner: &KeyNode,
        out: &mut Vec<u8>,
        depth: usize,
    ) -> Result<(), CratestackError> {
        self.expect(b'[')?;
        out.push(b'[');
        if self.peek()? == b']' {
            self.pos += 1;
            out.push(b']');
            return Ok(());
        }
        loop {
            self.value(inner, out, depth + 1)?;
            match self.peek()? {
                b',' => {
                    self.pos += 1;
                    out.push(b',');
                }
                b']' => {
                    self.pos += 1;
                    out.push(b']');
                    return Ok(());
                }
                _ => return Err(fail("expected `,` or `]`")),
            }
        }
    }
}
