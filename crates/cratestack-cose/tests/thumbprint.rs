//! RFC 9679 thumbprints and the `kid` derived from them.

mod common;

use common::{unhex, unhex32};
use cratestack_cose::CoseVerifyKey;
use cratestack_cose::thumbprint::{
    ec2_p256_thumbprint, kid_from_thumbprint, okp_ed25519_thumbprint, symmetric_thumbprint,
};
use sha2::{Digest, Sha256};

/// RFC 9679 §6, the specification's own example (an EC2 P-256 key).
const RFC9679_X: &str = "65eda5a12577c2bae829437fe338701a10aaa375e1bb5b5de108de439c08551d";
const RFC9679_Y: &str = "1e52ed75701163f7f9e40ddf9f341b3dc9ba860af7e0ca7ca7e9eecd0084d19c";
const RFC9679_THUMBPRINT: &str = "496bd8afadf307e5b08c64b0421bf9dc01528a344a43bda88fadd1669da253ec";

#[test]
fn rfc9679_section_6_example() {
    let thumbprint = ec2_p256_thumbprint(&unhex32(RFC9679_X), &unhex32(RFC9679_Y));
    assert_eq!(thumbprint, unhex32(RFC9679_THUMBPRINT));
}

#[test]
fn rfc9679_example_through_a_typed_key() {
    // The same key, entered as an uncompressed SEC1 point: the typed key's
    // thumbprint must come out identical, so a key's `kid` does not depend
    // on how it was loaded.
    let mut sec1 = vec![0x04];
    sec1.extend(unhex(RFC9679_X));
    sec1.extend(unhex(RFC9679_Y));
    let key = CoseVerifyKey::p256_sec1(&sec1).expect("the RFC example is a valid point");
    assert_eq!(key.thumbprint(), unhex32(RFC9679_THUMBPRINT));
    assert_eq!(key.kid().to_vec(), unhex("496bd8afadf307e5"));

    // Compressed form (0x02/0x03 by y parity): same thumbprint.
    let parity = unhex(RFC9679_Y)[31] & 1;
    let mut compressed = vec![0x02 | parity];
    compressed.extend(unhex(RFC9679_X));
    let key = CoseVerifyKey::p256_sec1(&compressed).expect("compressed point");
    assert_eq!(key.thumbprint(), unhex32(RFC9679_THUMBPRINT));
}

/// No published OKP example exists in RFC 9679, so the deterministic
/// encoding is written out by hand here and hashed independently.
#[test]
fn okp_ed25519_hand_encoded() {
    let x = unhex32("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
    // {1: 1, -1: 6, -2: h'…'}: A3 01 01 20 06 21 58 20 ‖ x
    let mut encoded = unhex("a3 01 01 20 06 21 58 20");
    encoded.extend_from_slice(&x);
    let expected: [u8; 32] = Sha256::digest(&encoded).into();
    assert_eq!(okp_ed25519_thumbprint(&x), expected);
    let key = CoseVerifyKey::ed25519(&x).expect("RFC 8032 test 1 public key");
    assert_eq!(key.thumbprint(), expected);
}

#[test]
fn symmetric_hand_encoded() {
    let k = [0x42_u8; 32];
    // {1: 4, -1: h'…'}: A2 01 04 20 58 20 ‖ k
    let mut encoded = unhex("a2 01 04 20 58 20");
    encoded.extend_from_slice(&k);
    let expected: [u8; 32] = Sha256::digest(&encoded).into();
    assert_eq!(symmetric_thumbprint(&k), expected);
    assert_eq!(
        CoseVerifyKey::hmac(cratestack_cose::CoseAlg::Hmac256_256, k.to_vec())
            .expect("32 bytes")
            .thumbprint(),
        expected
    );
}

#[test]
fn kid_is_the_first_eight_bytes() {
    let thumbprint = unhex32(RFC9679_THUMBPRINT);
    assert_eq!(kid_from_thumbprint(&thumbprint), thumbprint[..8]);
}

#[test]
fn different_key_types_never_share_a_thumbprint_for_the_same_bytes() {
    // The `kty` is inside the hashed structure, so 32 bytes read as an
    // Ed25519 point and as an HMAC secret give unrelated `kid`s.
    let bytes = unhex32("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
    assert_ne!(okp_ed25519_thumbprint(&bytes), symmetric_thumbprint(&bytes));
}
