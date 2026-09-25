//! The wire sizes ADR 0006 §3 measured, asserted against a byte-level
//! breakdown computed by hand, not against the vector files.
//!
//! With the 112-byte payment payload and an 8-byte `kid`:
//!
//! ```text
//!                                   Mac0 64   Mac0 256   Sign1 req   Sign1 resp
//! tag (0xd1 / 0xd2)                      1          1           1            1
//! array head 0x84                        1          1           1            1
//! protected bstr head                    2          2           2            1
//!   map head                             1          1           1            1
//!   1: alg                               2          2           2            2
//!   4: kid bstr(8)                      10         10          10           10
//!   15: { 6: iat(uint32), 7: cti }   1+1+1+5+1+(1+cti)             (absent)
//! unprotected 0xa0                       1          1           1            1
//! payload bstr head 0x58 0x70            2          2           2            2
//! payload                              112        112         112          112
//! signature/tag bstr               1+8=9    2+32=34     2+64=66      2+64=66
//! ```
//!
//! A 16-byte `cti` makes the claims map 26 bytes and the protected content
//! 39 (head `0x58 0x27`); a 2-byte counter makes them 12 and 25 (head
//! `0x58 0x19`). Both are over 23, so the protected head is 2 bytes either
//! way, and the difference is exactly 14 bytes. A response's protected
//! content is 13 bytes (head `0x4d`, 1 byte).

mod common;

use common::fixture::payment_bytes;
use common::{CTI_2, CTI_16, IAT, rpc_request};
use cratestack_cose::CoseAlg;

async fn request_len(alg: CoseAlg, cti: &str) -> usize {
    common::client(alg, IAT, cti)
        .seal_request(&payment_bytes(), &rpc_request())
        .await
        .expect("seal")
        .len()
}

async fn response_len(alg: CoseAlg) -> usize {
    let response = common::response_to(&rpc_request(), b"request", 200);
    common::server(alg, IAT)
        .seal_response(&payment_bytes(), &response)
        .await
        .expect("seal")
        .len()
}

#[tokio::test]
async fn sixteen_byte_cti_sizes() {
    assert_eq!(request_len(CoseAlg::Hmac256_64, CTI_16).await, 167);
    assert_eq!(request_len(CoseAlg::Hmac256_256, CTI_16).await, 192);
    assert_eq!(request_len(CoseAlg::Ed25519, CTI_16).await, 224);
    assert_eq!(request_len(CoseAlg::Esp256, CTI_16).await, 224);
}

/// ADR 0006 §3's table, measured by the proof of concept with a 2-byte
/// counter `cti`: 153 / 178 / 210.
#[tokio::test]
async fn two_byte_cti_sizes_match_the_adr_measurements() {
    assert_eq!(request_len(CoseAlg::Hmac256_64, CTI_2).await, 153);
    assert_eq!(request_len(CoseAlg::Hmac256_256, CTI_2).await, 178);
    assert_eq!(request_len(CoseAlg::Ed25519, CTI_2).await, 210);
    assert_eq!(request_len(CoseAlg::Esp256, CTI_2).await, 210);
}

/// ADR 0006 §3: a Sign1 response (`kid` only) is 197 bytes. Mac0
/// responses were not measured by the ADR; 140 / 165 follow from the
/// same breakdown.
#[tokio::test]
async fn response_sizes() {
    assert_eq!(response_len(CoseAlg::Ed25519).await, 197);
    assert_eq!(response_len(CoseAlg::Esp256).await, 197);
    assert_eq!(response_len(CoseAlg::Hmac256_64).await, 140);
    assert_eq!(response_len(CoseAlg::Hmac256_256).await, 165);
}

/// The breakdown itself, on the bytes: the fixed prefix of a Sign1 request.
#[tokio::test]
async fn sign1_request_prefix_is_as_broken_down() {
    let sealed = common::sealed_request(CoseAlg::Ed25519, &rpc_request()).await;
    let kid = common::ed25519().verify_key().kid();
    let mut expected = vec![0xd2, 0x84, 0x58, 0x27, 0xa3, 0x01, 0x32, 0x04, 0x48];
    expected.extend_from_slice(&kid);
    expected.extend_from_slice(&[0x0f, 0xa2, 0x06, 0x1a]);
    expected.extend_from_slice(&u32::try_from(IAT).expect("u32").to_be_bytes());
    expected.extend_from_slice(&[0x07, 0x50]);
    expected.extend_from_slice(&common::unhex(CTI_16));
    expected.extend_from_slice(&[0xa0, 0x58, 0x70]);
    assert_eq!(&sealed[..expected.len()], expected.as_slice());
    assert_eq!(
        &sealed[expected.len()..expected.len() + 112],
        payment_bytes()
    );
    assert_eq!(
        &sealed[expected.len() + 112..expected.len() + 114],
        &[0x58, 0x40]
    );
    assert_eq!(sealed.len(), expected.len() + 114 + 64);
}
