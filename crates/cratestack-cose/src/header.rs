//! The protected header (ADR 0006 §3).
//!
//! ```cddl
//! protected = {
//!   1 => int,            ; alg
//!   4 => bstr .size 8,   ; kid
//!   ? 15 => {            ; CWT claims (RFC 9597): requests only
//!     6 => uint,         ;   iat, seconds
//!     7 => bstr,         ;   cti
//!   }
//! }
//! ```
//!
//! The parser accepts exactly the deterministic encoding the writer
//! produces. Labels must appear in ascending order, which also rules out
//! duplicates; any other label, any other value type and any trailing byte
//! is a [`Reject`]. In P0 (`nonce` mode) label 15 carries **both** `iat`
//! and `cti`. The CDDL marks each optional to leave room for P2's `window`
//! mode, which will relax this parser deliberately, not by accident.

use std::ops::Range;

use crate::alg::CoseAlg;
use crate::cbor::read::Reader;
use crate::cbor::write::{self, MAJOR_MAP, MAJOR_UINT};
use crate::error::Reject;
use crate::thumbprint::KID_LEN;

const LABEL_ALG: u64 = 1;
const LABEL_KID: u64 = 4;
const LABEL_CWT_CLAIMS: u64 = 15;
const CLAIM_IAT: u64 = 6;
const CLAIM_CTI: u64 = 7;

/// Whether `len` is an accepted `cti` length (§3, §5): a device counter of
/// 1 to 4 bytes, or 16 random bytes. The sealer enforces the same rule on
/// its `cti` source, so a message this crate emits always parses.
pub(crate) const fn cti_len_ok(len: usize) -> bool {
    matches!(len, 1..=4 | 16)
}

/// Encode a protected header. `claims` is `Some((iat, cti))` for a request
/// and `None` for a response.
pub(crate) fn encode(alg: CoseAlg, kid: &[u8], claims: Option<(u64, &[u8])>) -> Vec<u8> {
    let mut out = Vec::with_capacity(48);
    write::head(&mut out, MAJOR_MAP, if claims.is_some() { 3 } else { 2 });
    write::head(&mut out, MAJOR_UINT, LABEL_ALG);
    write::int(&mut out, alg.id());
    write::head(&mut out, MAJOR_UINT, LABEL_KID);
    write::bstr(&mut out, kid);
    if let Some((iat, cti)) = claims {
        write::head(&mut out, MAJOR_UINT, LABEL_CWT_CLAIMS);
        write::head(&mut out, MAJOR_MAP, 2);
        write::head(&mut out, MAJOR_UINT, CLAIM_IAT);
        write::head(&mut out, MAJOR_UINT, iat);
        write::head(&mut out, MAJOR_UINT, CLAIM_CTI);
        write::bstr(&mut out, cti);
    }
    out
}

/// A parsed protected header. Ranges index into the protected bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Protected {
    pub(crate) alg: CoseAlg,
    pub(crate) kid: Range<usize>,
    /// `(iat, cti range)` when label 15 is present.
    pub(crate) claims: Option<(u64, Range<usize>)>,
}

/// Parse the protected header bytes, strictly.
pub(crate) fn parse(bytes: &[u8]) -> Result<Protected, Reject> {
    let mut reader = Reader::new(bytes);
    let entries = reader.expect(MAJOR_MAP)?;
    if entries != 2 && entries != 3 {
        return Err(Reject);
    }
    if reader.uint()? != LABEL_ALG {
        return Err(Reject);
    }
    let alg = CoseAlg::from_id(reader.int()?).ok_or(Reject)?;
    if reader.uint()? != LABEL_KID {
        return Err(Reject);
    }
    let kid = reader.bstr_range()?;
    if kid.len() != KID_LEN {
        return Err(Reject);
    }
    let claims = if entries == 3 {
        if reader.uint()? != LABEL_CWT_CLAIMS {
            return Err(Reject);
        }
        Some(parse_claims(&mut reader)?)
    } else {
        None
    };
    if !reader.is_at_end() {
        return Err(Reject);
    }
    Ok(Protected { alg, kid, claims })
}

fn parse_claims(reader: &mut Reader<'_>) -> Result<(u64, Range<usize>), Reject> {
    if reader.expect(MAJOR_MAP)? != 2 || reader.uint()? != CLAIM_IAT {
        return Err(Reject);
    }
    let iat = reader.uint()?;
    if reader.uint()? != CLAIM_CTI {
        return Err(Reject);
    }
    let cti = reader.bstr_range()?;
    if !cti_len_ok(cti.len()) {
        return Err(Reject);
    }
    Ok((iat, cti))
}
