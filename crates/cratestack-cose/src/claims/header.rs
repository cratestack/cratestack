//! The protected header of a claim object.
//!
//! ```cddl
//! protected = {
//!   1  => int,                ; alg: a signature algorithm
//!   4  => bstr .size 8,       ; kid
//!   ? 15 => {                 ; CWT claims (RFC 9597), ascending, non-empty
//!     ? 3 => tstr,            ;   aud
//!     ? 4 => uint,            ;   exp
//!     ? 6 => uint,            ;   iat
//!     ? 7 => bstr,            ;   cti
//!   },
//!   16 => tstr,               ; typ (RFC 9596): required, the domain separator
//! }
//! ```
//!
//! Labels ascend, which is both the deterministic order (RFC 8949 §4.2.1:
//! every label here encodes in one byte) and the duplicate check. The
//! transport envelope's header parser refuses label 16, and this one
//! refuses a header without it, so neither opens the other's messages.

use std::ops::Range;

use super::types::{CwtClaims, MAX_AUD_LEN, MAX_CTI_LEN, MAX_TYP_LEN};
use crate::alg::CoseAlg;
use crate::cbor::read::Reader;
use crate::cbor::write::{self, MAJOR_MAP, MAJOR_UINT};
use crate::error::Reject;
use crate::header::{LABEL_ALG, LABEL_CWT_CLAIMS, LABEL_KID};
use crate::thumbprint::KID_LEN;

const LABEL_TYP: u64 = 16;
const CLAIM_AUD: u64 = 3;
const CLAIM_EXP: u64 = 4;
const CLAIM_IAT: u64 = 6;
const CLAIM_CTI: u64 = 7;

/// Whether the claims are within the bounds both sides enforce; `Err` names
/// the first violated bound, so the signer can say which.
pub(super) fn check_claims(claims: &CwtClaims) -> Result<(), &'static str> {
    if let Some(aud) = &claims.aud
        && (aud.is_empty() || aud.len() > MAX_AUD_LEN)
    {
        return Err("aud must be 1 to MAX_AUD_LEN bytes");
    }
    if let Some(cti) = &claims.cti
        && (cti.is_empty() || cti.len() > MAX_CTI_LEN)
    {
        return Err("cti must be 1 to MAX_CTI_LEN bytes");
    }
    Ok(())
}

pub(super) fn typ_ok(typ: &str) -> bool {
    !typ.is_empty() && typ.len() <= MAX_TYP_LEN
}

fn has_claims(claims: &CwtClaims) -> bool {
    claims.aud.is_some() || claims.exp.is_some() || claims.iat.is_some() || claims.cti.is_some()
}

/// Encode the header. The caller has checked `typ` and `claims`.
pub(super) fn encode(alg: CoseAlg, kid: &[u8], typ: &str, claims: &CwtClaims) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + typ.len());
    write::head(&mut out, MAJOR_MAP, if has_claims(claims) { 4 } else { 3 });
    write::head(&mut out, MAJOR_UINT, LABEL_ALG);
    write::int(&mut out, alg.id());
    write::head(&mut out, MAJOR_UINT, LABEL_KID);
    write::bstr(&mut out, kid);
    if has_claims(claims) {
        write::head(&mut out, MAJOR_UINT, LABEL_CWT_CLAIMS);
        let entries = [
            claims.aud.is_some(),
            claims.exp.is_some(),
            claims.iat.is_some(),
            claims.cti.is_some(),
        ];
        let count: u64 = entries.iter().map(|&present| u64::from(present)).sum();
        write::head(&mut out, MAJOR_MAP, count);
        if let Some(aud) = &claims.aud {
            write::head(&mut out, MAJOR_UINT, CLAIM_AUD);
            write::tstr(&mut out, aud);
        }
        for (label, value) in [(CLAIM_EXP, claims.exp), (CLAIM_IAT, claims.iat)] {
            if let Some(value) = value {
                write::head(&mut out, MAJOR_UINT, label);
                write::head(&mut out, MAJOR_UINT, value);
            }
        }
        if let Some(cti) = &claims.cti {
            write::head(&mut out, MAJOR_UINT, CLAIM_CTI);
            write::bstr(&mut out, cti);
        }
    }
    write::head(&mut out, MAJOR_UINT, LABEL_TYP);
    write::tstr(&mut out, typ);
    out
}

/// A parsed header. `kid` indexes into the protected bytes.
pub(super) struct Parsed {
    pub(super) alg: CoseAlg,
    pub(super) kid: Range<usize>,
    pub(super) claims: CwtClaims,
    pub(super) typ: String,
}

/// Parse the protected header bytes, strictly.
pub(super) fn parse(bytes: &[u8]) -> Result<Parsed, Reject> {
    let mut reader = Reader::new(bytes);
    let entries = reader.expect(MAJOR_MAP)?;
    if entries != 3 && entries != 4 {
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
    let claims = if entries == 4 {
        if reader.uint()? != LABEL_CWT_CLAIMS {
            return Err(Reject);
        }
        parse_claims(&mut reader)?
    } else {
        CwtClaims::default()
    };
    if reader.uint()? != LABEL_TYP {
        return Err(Reject);
    }
    let typ = reader.tstr()?;
    if !typ_ok(typ) || !reader.is_at_end() {
        return Err(Reject);
    }
    Ok(Parsed {
        alg,
        kid,
        claims,
        typ: typ.to_owned(),
    })
}

fn parse_claims(reader: &mut Reader<'_>) -> Result<CwtClaims, Reject> {
    let entries = reader.expect(MAJOR_MAP)?;
    if !(1..=4).contains(&entries) {
        return Err(Reject);
    }
    let mut claims = CwtClaims::default();
    let mut previous = 0;
    for _ in 0..entries {
        let label = reader.uint()?;
        if label <= previous {
            return Err(Reject);
        }
        previous = label;
        match label {
            CLAIM_AUD => claims.aud = Some(reader.tstr()?.to_owned()),
            CLAIM_EXP => claims.exp = Some(reader.uint()?),
            CLAIM_IAT => claims.iat = Some(reader.uint()?),
            CLAIM_CTI => {
                claims.cti = Some(reader.bstr()?.to_vec());
            }
            _ => return Err(Reject),
        }
    }
    check_claims(&claims).map_err(|_| Reject)?;
    Ok(claims)
}
