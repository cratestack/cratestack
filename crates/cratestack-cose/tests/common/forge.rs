//! Build messages the crate itself would never emit, *correctly signed*,
//! so a rejection can be pinned on the strictness check under test and not
//! on a broken signature.
//!
//! The to-be-signed bytes come from `coset` (`sig_structure_data` /
//! `mac_structure_data`), not from the crate, so these helpers are an
//! independent implementation of what they sign.

use coset::{MacContext, ProtectedHeader, SignatureContext};
use ed25519_dalek::Signer as _;
use hmac::{KeyInit, Mac};

use super::{ED25519_SEED, HMAC_SECRET, P256_SCALAR, unhex32};

/// Tags as single bytes: `0xd2` = tag 18 (Sign1), `0xd1` = tag 17 (Mac0).
pub const TAG_SIGN1: u8 = 0xd2;
pub const TAG_MAC0: u8 = 0xd1;

fn raw(protected: &[u8]) -> ProtectedHeader {
    ProtectedHeader {
        original_data: Some(protected.to_vec()),
        header: Default::default(),
    }
}

pub fn sign1_tbs(protected: &[u8], aad: &[u8], payload: &[u8]) -> Vec<u8> {
    coset::sig_structure_data(
        SignatureContext::CoseSign1,
        raw(protected),
        None,
        aad,
        payload,
    )
}

pub fn mac0_tbs(protected: &[u8], aad: &[u8], payload: &[u8]) -> Vec<u8> {
    coset::mac_structure_data(MacContext::CoseMac0, raw(protected), aad, payload)
}

pub fn ed25519_sign(tbs: &[u8]) -> Vec<u8> {
    ed25519_dalek::SigningKey::from_bytes(&ED25519_SEED)
        .sign(tbs)
        .to_bytes()
        .to_vec()
}

pub fn p256_sign(tbs: &[u8]) -> Vec<u8> {
    let key = p256::ecdsa::SigningKey::from_slice(&unhex32(P256_SCALAR)).expect("scalar");
    let signature: p256::ecdsa::Signature = key.sign(tbs);
    signature.to_bytes().to_vec()
}

/// HMAC-SHA-256 with an arbitrary key, truncated to `len`.
pub fn hmac_with(key: &[u8], tbs: &[u8], len: usize) -> Vec<u8> {
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key).expect("any key length");
    mac.update(tbs);
    mac.finalize().into_bytes()[..len].to_vec()
}

pub fn hmac_sign(tbs: &[u8], len: usize) -> Vec<u8> {
    hmac_with(&HMAC_SECRET, tbs, len)
}

/// A minimal-head definite `bstr`.
pub fn bstr(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 9);
    match bytes.len() {
        len @ 0..=23 => out.push(0x40 | len as u8),
        len @ 24..=0xff => out.extend_from_slice(&[0x58, len as u8]),
        len => {
            out.push(0x59);
            out.extend_from_slice(&u16::try_from(len).expect("test sizes").to_be_bytes());
        }
    }
    out.extend_from_slice(bytes);
    out
}

/// Assemble `tag [protected, <unprotected>, payload, signature]` with the
/// unprotected header given raw, so a test can put anything there.
pub fn assemble(
    tag: u8,
    protected: &[u8],
    unprotected: &[u8],
    payload: &[u8],
    sig: &[u8],
) -> Vec<u8> {
    let mut out = vec![tag, 0x84];
    out.extend(bstr(protected));
    out.extend_from_slice(unprotected);
    out.extend(bstr(payload));
    out.extend(bstr(sig));
    out
}

/// A canonical request protected header, hand-encoded:
/// `{1: alg, 4: kid, 15: {6: iat, 7: cti}}`.
pub fn request_protected(alg_id: i8, kid: &[u8], iat: u32, cti: &[u8]) -> Vec<u8> {
    let mut out = vec![0xa3, 0x01];
    out.push(int_byte(alg_id));
    out.push(0x04);
    out.extend(bstr(kid));
    out.extend_from_slice(&[0x0f, 0xa2, 0x06, 0x1a]);
    out.extend_from_slice(&iat.to_be_bytes());
    out.push(0x07);
    out.extend(bstr(cti));
    out
}

/// A one-byte CBOR integer (-24..=23).
pub fn int_byte(value: i8) -> u8 {
    if value >= 0 {
        u8::try_from(value).expect("small")
    } else {
        0x20 | u8::try_from(-1 - i16::from(value)).expect("small")
    }
}

/// A Sign1 request over `protected`, signed with the Ed25519 test key.
pub fn ed25519_request(protected: &[u8], aad: &[u8], payload: &[u8]) -> Vec<u8> {
    let sig = ed25519_sign(&sign1_tbs(protected, aad, payload));
    assemble(TAG_SIGN1, protected, &[0xa0], payload, &sig)
}

/// Where the parts of a well-formed tagged message sit, found by reading
/// its heads (test-side, independent of the crate's parser). Handles only
/// the one-byte tag and the 1- or 2-byte `bstr` heads the crate emits.
#[derive(Debug, Clone)]
pub struct Layout {
    pub protected: std::ops::Range<usize>,
    pub unprotected: usize,
    pub payload: std::ops::Range<usize>,
    pub signature: std::ops::Range<usize>,
}

fn read_bstr(bytes: &[u8], at: usize) -> std::ops::Range<usize> {
    match bytes[at] {
        head @ 0x40..=0x57 => at + 1..at + 1 + usize::from(head - 0x40),
        0x58 => at + 2..at + 2 + usize::from(bytes[at + 1]),
        other => panic!("unexpected head {other:#x} at {at}"),
    }
}

pub fn layout(bytes: &[u8]) -> Layout {
    assert!(matches!(bytes[0], TAG_SIGN1 | TAG_MAC0) && bytes[1] == 0x84);
    let protected = read_bstr(bytes, 2);
    let unprotected = protected.end;
    let payload = read_bstr(bytes, unprotected + 1);
    let signature = read_bstr(bytes, payload.end);
    assert_eq!(signature.end, bytes.len());
    Layout {
        protected,
        unprotected,
        payload,
        signature,
    }
}

/// Re-assemble `bytes` with its unprotected header replaced by `raw`.
pub fn with_unprotected(bytes: &[u8], raw: &[u8]) -> Vec<u8> {
    let layout = layout(bytes);
    let mut out = bytes[..layout.unprotected].to_vec();
    out.extend_from_slice(raw);
    out.extend_from_slice(&bytes[layout.unprotected + 1..]);
    out
}
