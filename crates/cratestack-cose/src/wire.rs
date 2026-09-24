//! Parsing the outer message (RFC 9052 §4.2, §6.2) by hand; see the private
//! `cbor` module for why. The sealer (`seal.rs`) emits the same shape, and
//! the to-be-signed structure is in `tbs.rs`.
//!
//! ```text
//! Tag(18 | 17) [ protected: bstr, unprotected: {}, payload: bstr, signature/tag: bstr ]
//! ```
//!
//! The parser accepts only that shape: the expected tag, a definite
//! 4-element array, a non-empty protected `bstr`, an unprotected header
//! that is exactly `0xa0`, an embedded payload (a detached `nil` payload is
//! rejected) and no trailing bytes. The unprotected check is the one that
//! matters most: nothing in it is authenticated, so any content there is
//! something an attacker could have added (§3: "nothing unauthenticated on
//! the wire").

use std::ops::Range;

use crate::alg::CoseMode;
use crate::cbor::read::Reader;
use crate::cbor::write::{EMPTY_MAP, MAJOR_ARRAY, MAJOR_TAG};
use crate::error::Reject;

/// Where each part of a parsed message sits in the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Parts {
    pub(crate) protected: Range<usize>,
    pub(crate) payload: Range<usize>,
    pub(crate) signature: Range<usize>,
}

/// Parse a tagged message of the given mode, strictly.
pub(crate) fn parse(mode: CoseMode, body: &[u8]) -> Result<Parts, Reject> {
    let mut reader = Reader::new(body);
    if reader.expect(MAJOR_TAG)? != mode.tag() {
        return Err(Reject);
    }
    if reader.expect(MAJOR_ARRAY)? != 4 {
        return Err(Reject);
    }
    let protected = reader.bstr_range()?;
    if protected.is_empty() {
        return Err(Reject);
    }
    reader.expect_byte(EMPTY_MAP)?;
    let payload = reader.bstr_range()?;
    let signature = reader.bstr_range()?;
    if !reader.is_at_end() {
        return Err(Reject);
    }
    Ok(Parts {
        protected,
        payload,
        signature,
    })
}
