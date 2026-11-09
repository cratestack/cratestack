//! Explicit nesting-depth guard for CBOR decoding (cratestack#1315).
//!
//! `minicbor-serde`'s deserializer recurses once per array, map and tag, with
//! no bound, so a few thousand `0x81` headers overflow the native stack
//! (SIGSEGV or abort, not catchable). RFC 8949 §5.1 asks decoders to bound
//! their resource use on untrusted input. [`check_depth`] walks the item with
//! its own `Vec` stack before serde sees it, so the cost of a hostile input is
//! heap, bounded by the limit, never call stack.

use cratestack_core::CratestackError;
use minicbor::Decoder;
use minicbor::data::Type;

/// Nesting levels a decode accepts unless the caller picks another limit.
pub const DEFAULT_MAX_DECODE_DEPTH: usize = 128;

/// Remaining children of an open container (`None` = indefinite, closed by a
/// break). A tag is a container of exactly one item.
type Frame = Option<u64>;

/// Refuses the first CBOR item in `bytes` if arrays, maps and tags nest more
/// than `max_depth` levels. A scalar has depth 0. Malformed or truncated input
/// is not this function's business: it returns `Ok` and lets the real decoder
/// report it, having verified every level it reached.
pub fn check_depth(bytes: &[u8], max_depth: usize) -> Result<(), CratestackError> {
    let mut decoder = Decoder::new(bytes);
    let mut stack: Vec<Frame> = Vec::new();
    loop {
        let Ok(kind) = decoder.datatype() else {
            return Ok(());
        };
        let opened = match kind {
            Type::Array => decoder.array().map(|n| n.map(|n| (n, true))),
            Type::ArrayIndef => decoder.array().map(|_| Some((u64::MAX, false))),
            Type::Map => decoder
                .map()
                .map(|n| n.map(|n| (n.saturating_mul(2), true))),
            Type::MapIndef => decoder.map().map(|_| Some((u64::MAX, false))),
            Type::Tag => decoder.tag().map(|_| Some((1, true))),
            Type::Break => {
                decoder.set_position(decoder.position() + 1);
                stack.pop();
                if complete_item(&mut stack) {
                    return Ok(());
                }
                continue;
            }
            _ => match decoder.skip() {
                Ok(()) => {
                    if complete_item(&mut stack) {
                        return Ok(());
                    }
                    continue;
                }
                Err(_) => return Ok(()),
            },
        };
        match opened {
            Err(_) => return Ok(()),
            // Empty definite container: an item with no children.
            Ok(None) | Ok(Some((0, true))) => {
                if complete_item(&mut stack) {
                    return Ok(());
                }
            }
            Ok(Some((count, definite))) => {
                if stack.len() >= max_depth {
                    return Err(too_deep(max_depth));
                }
                stack.push(definite.then_some(count));
            }
        }
    }
}

/// Marks one item finished in the enclosing container, closing every
/// definite container that was waiting only on it. Returns `true` once the
/// top-level item is complete.
fn complete_item(stack: &mut Vec<Frame>) -> bool {
    while let Some(Some(remaining)) = stack.last_mut() {
        *remaining -= 1;
        if *remaining > 0 {
            return false;
        }
        stack.pop();
    }
    stack.is_empty()
}

pub(crate) fn too_deep(max_depth: usize) -> CratestackError {
    CratestackError::Codec(format!(
        "failed to decode CBOR body: nesting too deep (maximum depth is {max_depth})"
    ))
}
