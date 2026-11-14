//! A P-256 `SubjectPublicKeyInfo` (RFC 5480) read down to its SEC1 point.
//!
//! Android's `PublicKey.getEncoded()` returns this form, so a keystore
//! signer's public key arrives as SPKI as often as it arrives as a bare
//! point. Parsed strictly and here, in one place, instead of by a byte
//! prefix strip in each client: the only accepted shape is
//!
//! ```text
//! SEQUENCE {
//!   SEQUENCE { OID id-ecPublicKey, OID prime256v1 }
//!   BIT STRING { 0 unused bits, 0x04 ‖ x ‖ y }
//! }
//! ```
//!
//! in definite, minimal DER, with nothing after it. Another algorithm or
//! curve, a compressed point, explicit curve parameters, and trailing bytes
//! are each refused.

/// `1.2.840.10045.2.1`, id-ecPublicKey.
const ID_EC_PUBLIC_KEY: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
/// `1.2.840.10045.3.1.7`, prime256v1 (secp256r1, P-256).
const PRIME256V1: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];

const SEQUENCE: u8 = 0x30;
const OID: u8 = 0x06;
const BIT_STRING: u8 = 0x03;

/// The uncompressed SEC1 point (`0x04 ‖ x ‖ y`) inside `der`, or `None` for
/// anything but the exact P-256 shape above. Whether the point is on the
/// curve is the caller's check (`CoseVerifyKey::p256_sec1`).
pub(super) fn p256_point(der: &[u8]) -> Option<&[u8]> {
    let (spki, rest) = element(der, SEQUENCE)?;
    if !rest.is_empty() {
        return None;
    }
    let (algorithm, rest) = element(spki, SEQUENCE)?;
    let (bits, rest) = element(rest, BIT_STRING)?;
    if !rest.is_empty() {
        return None;
    }
    let (oid, parameters) = element(algorithm, OID)?;
    let (curve, rest) = element(parameters, OID)?;
    if oid != ID_EC_PUBLIC_KEY || curve != PRIME256V1 || !rest.is_empty() {
        return None;
    }
    // Zero unused bits, then an uncompressed point.
    match bits.split_first() {
        Some((0, point)) if point.len() == 65 && point[0] == 0x04 => Some(point),
        _ => None,
    }
}

/// One DER element of `tag` at the front of `input`: its content, and what
/// follows. Lengths must be definite and minimal.
fn element(input: &[u8], tag: u8) -> Option<(&[u8], &[u8])> {
    let (&found, input) = input.split_first()?;
    let (&first, input) = input.split_first()?;
    if found != tag {
        return None;
    }
    let (len, input) = match first {
        0..=0x7f => (usize::from(first), input),
        // Long form, one or two length bytes: a P-256 SPKI is 91 bytes.
        0x81 | 0x82 => {
            let count = usize::from(first & 0x7f);
            let (bytes, input) = input.split_at_checked(count)?;
            let len = bytes
                .iter()
                .fold(0usize, |acc, b| (acc << 8) | usize::from(*b));
            // Minimal: the long form only for what the short form cannot hold.
            if len < 0x80 || (count == 2 && len < 0x100) {
                return None;
            }
            (len, input)
        }
        _ => return None,
    };
    input.split_at_checked(len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoseVerifyKey, P256Signer};

    fn point() -> [u8; 65] {
        P256Signer::from_scalar(&[7; 32])
            .expect("scalar")
            .verify_key()
            .p256_sec1_uncompressed()
            .expect("P-256 key")
    }

    /// An SPKI around `point`, with the algorithm OID and curve OID given.
    fn spki(alg: &[u8], curve: &[u8], point: &[u8]) -> Vec<u8> {
        let tlv = |tag: u8, body: &[u8]| {
            let mut out = vec![tag, body.len() as u8];
            out.extend_from_slice(body);
            out
        };
        let mut algorithm = tlv(OID, alg);
        algorithm.extend(tlv(OID, curve));
        let mut bits = vec![0];
        bits.extend_from_slice(point);
        let mut body = tlv(SEQUENCE, &algorithm);
        body.extend(tlv(BIT_STRING, &bits));
        tlv(SEQUENCE, &body)
    }

    fn valid() -> Vec<u8> {
        spki(ID_EC_PUBLIC_KEY, PRIME256V1, &point())
    }

    #[test]
    fn a_valid_spki_is_its_sec1_form() {
        let der = valid();
        assert_eq!(der.len(), 91);
        assert_eq!(p256_point(&der), Some(point().as_slice()));
        let from_spki = CoseVerifyKey::p256_spki(&der).expect("spki");
        let from_sec1 = CoseVerifyKey::p256_sec1(&point()).expect("sec1");
        assert_eq!(from_spki, from_sec1);
        assert_eq!(from_spki.kid(), from_sec1.kid());
        // The auto-detecting constructor takes either spelling.
        assert_eq!(CoseVerifyKey::p256_public(&der).expect("der"), from_sec1);
        assert_eq!(
            CoseVerifyKey::p256_public(&point()).expect("sec1"),
            from_sec1
        );
    }

    #[test]
    fn a_wrong_curve_is_refused() {
        // P-384 (secp384r1, 1.3.132.0.34), same structure.
        let der = spki(ID_EC_PUBLIC_KEY, &[0x2b, 0x81, 0x04, 0x00, 0x22], &point());
        assert!(p256_point(&der).is_none());
        assert!(CoseVerifyKey::p256_spki(&der).is_err());
        // secp256k1 (1.3.132.0.10).
        let der = spki(ID_EC_PUBLIC_KEY, &[0x2b, 0x81, 0x04, 0x00, 0x0a], &point());
        assert!(CoseVerifyKey::p256_spki(&der).is_err());
    }

    #[test]
    fn a_wrong_algorithm_is_refused() {
        // id-Ed25519 (1.3.101.112) where id-ecPublicKey belongs.
        let der = spki(&[0x2b, 0x65, 0x70], PRIME256V1, &point());
        assert!(CoseVerifyKey::p256_spki(&der).is_err());
    }

    #[test]
    fn a_compressed_point_is_refused() {
        let mut compressed = vec![if point()[64] & 1 == 0 { 0x02 } else { 0x03 }];
        compressed.extend_from_slice(&point()[1..33]);
        let der = spki(ID_EC_PUBLIC_KEY, PRIME256V1, &compressed);
        assert!(CoseVerifyKey::p256_spki(&der).is_err());
        // An uncompressed-looking point of the wrong length too.
        let der = spki(ID_EC_PUBLIC_KEY, PRIME256V1, &point()[..64]);
        assert!(CoseVerifyKey::p256_spki(&der).is_err());
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let mut der = valid();
        der.push(0);
        assert!(CoseVerifyKey::p256_spki(&der).is_err());
        // Trailing bytes inside the outer sequence, with its length fixed up.
        let mut inside = valid();
        inside.push(0);
        inside[1] += 1;
        assert!(CoseVerifyKey::p256_spki(&inside).is_err());
    }

    #[test]
    fn a_truncated_or_off_curve_spki_is_refused() {
        let der = valid();
        assert!(CoseVerifyKey::p256_spki(&der[..90]).is_err());
        assert!(CoseVerifyKey::p256_spki(&[]).is_err());
        // Well-formed, but the point is not on the curve.
        let mut off = point();
        off[64] ^= 1;
        let der = spki(ID_EC_PUBLIC_KEY, PRIME256V1, &off);
        assert!(CoseVerifyKey::p256_spki(&der).is_err());
    }
}
