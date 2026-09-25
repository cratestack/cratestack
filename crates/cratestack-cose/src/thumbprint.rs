//! RFC 9679 COSE Key Thumbprints, and the 8-byte `kid` derived from them
//! (ADR 0006 §3).
//!
//! A thumbprint is SHA-256 over the deterministic CBOR encoding (RFC 8949
//! §4.2.1) of a COSE_Key holding **only** the key type's required
//! parameters (RFC 9679 §4). Deterministic map order sorts keys by their
//! encoded bytes, so `1` (`0x01`) precedes `-1` (`0x20`), `-2` (`0x21`) and
//! `-3` (`0x22`).
//!
//! Symmetric keys: RFC 9679 §7 allows their thumbprints only for keys with
//! enough entropy to rule out precomputed tables. [`HmacSecret`] refuses
//! secrets shorter than 32 bytes for exactly that reason (P0 scoping
//! decision), and only the first 8 bytes of the digest ever reach the wire.
//!
//! [`HmacSecret`]: crate::HmacSecret

use sha2::{Digest, Sha256};

use crate::cbor::write::{self, MAJOR_MAP};

/// The length of a `kid` on the wire (§3: `bstr .size 8`).
pub const KID_LEN: usize = 8;

/// COSE key type OKP (`kty` 1), curve Ed25519 (`crv` 6).
pub fn okp_ed25519_thumbprint(x: &[u8; 32]) -> [u8; 32] {
    let mut key = Vec::with_capacity(41);
    write::head(&mut key, MAJOR_MAP, 3);
    key.extend_from_slice(&[0x01, 0x01, 0x20, 0x06, 0x21]);
    write::bstr(&mut key, x);
    Sha256::digest(&key).into()
}

/// COSE key type EC2 (`kty` 2), curve P-256 (`crv` 1), from the
/// uncompressed coordinates (RFC 9679 §4.2 requires the uncompressed form).
pub fn ec2_p256_thumbprint(x: &[u8; 32], y: &[u8; 32]) -> [u8; 32] {
    let mut key = Vec::with_capacity(77);
    write::head(&mut key, MAJOR_MAP, 4);
    key.extend_from_slice(&[0x01, 0x02, 0x20, 0x01, 0x21]);
    write::bstr(&mut key, x);
    key.push(0x22);
    write::bstr(&mut key, y);
    Sha256::digest(&key).into()
}

/// COSE key type Symmetric (`kty` 4), from the secret `k`.
pub fn symmetric_thumbprint(k: &[u8]) -> [u8; 32] {
    let mut key = Vec::with_capacity(4 + write::bstr_len(k.len()));
    write::head(&mut key, MAJOR_MAP, 2);
    key.extend_from_slice(&[0x01, 0x04, 0x20]);
    write::bstr(&mut key, k);
    Sha256::digest(&key).into()
}

/// The `kid` for a thumbprint: its first [`KID_LEN`] bytes. The birthday
/// bound is about 2³² keys, and a resolver returns several candidates on a
/// collision (§3), so the full thumbprint, not the `kid`, is what names a
/// key unambiguously ([`Opened::key_thumbprint`](crate::Opened)).
pub fn kid_from_thumbprint(thumbprint: &[u8; 32]) -> [u8; KID_LEN] {
    let mut kid = [0; KID_LEN];
    kid.copy_from_slice(&thumbprint[..KID_LEN]);
    kid
}
