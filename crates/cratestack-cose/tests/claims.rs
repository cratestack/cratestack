//! Claim objects (`sign_claims` / `open_claims`): the round trip, what is
//! deliberately not checked, and every way the inputs can be wrong.

mod common;

use std::sync::Arc;

use common::backends::{FailingResolver, FixedResolver};
use cratestack_core::CratestackError;
use cratestack_cose::{
    ClaimsError, CoseAlg, CoseSigner, CwtClaims, Ed25519Signer, MAX_AUD_LEN, MAX_CTI_LEN,
    MAX_TYP_LEN, StaticVerifierResolver, UNAUTHENTICATED, open_claims, sign_claims,
};

pub const TYP: &str = "application/vnd.cratestack.test-grant+cwt";
const AAD: &[u8] = b"tenant-1|grant";
const PAYLOAD: &[u8] = b"grant: 5 sheep";

fn all_claims() -> CwtClaims {
    CwtClaims {
        aud: Some("vaam".to_owned()),
        exp: Some(1_790_003_600),
        iat: Some(1_790_000_000),
        cti: Some(vec![0xa5; 16]),
    }
}

/// Every subset of the four claims.
fn claim_subsets() -> Vec<CwtClaims> {
    (0..16u8)
        .map(|mask| {
            let full = all_claims();
            CwtClaims {
                aud: full.aud.filter(|_| mask & 1 != 0),
                exp: full.exp.filter(|_| mask & 2 != 0),
                iat: full.iat.filter(|_| mask & 4 != 0),
                cti: full.cti.filter(|_| mask & 8 != 0),
            }
        })
        .collect()
}

fn signers() -> Vec<Arc<dyn CoseSigner>> {
    vec![
        common::signer(CoseAlg::Ed25519),
        common::signer(CoseAlg::Esp256),
    ]
}

#[tokio::test]
async fn every_claim_subset_round_trips_under_both_signature_algorithms() {
    let resolver = common::resolver();
    for signer in signers() {
        for claims in claim_subsets() {
            let bytes = sign_claims(signer.as_ref(), TYP, &claims, PAYLOAD, AAD)
                .await
                .expect("sign");
            assert_eq!(bytes[0], 0xd2, "tag 18");
            let opened = open_claims(resolver.as_ref(), &bytes, TYP, AAD)
                .await
                .expect("open");
            assert_eq!(opened.claims, claims);
            assert_eq!(opened.payload, PAYLOAD);
            assert_eq!(opened.alg, signer.alg());
            assert_eq!(opened.kid.as_slice(), signer.kid());
        }
    }
}

#[tokio::test]
async fn empty_payload_and_empty_aad_are_legal() {
    let signer = common::ed25519();
    let bytes = sign_claims(&signer, TYP, &CwtClaims::default(), b"", b"")
        .await
        .expect("sign");
    let opened = open_claims(common::resolver().as_ref(), &bytes, TYP, b"")
        .await
        .expect("open");
    assert!(opened.payload.is_empty());
    assert_eq!(opened.claims, CwtClaims::default());
}

#[tokio::test]
async fn the_encoding_is_deterministic_and_ed25519_is_byte_stable() {
    let signer = common::ed25519();
    let a = sign_claims(&signer, TYP, &all_claims(), PAYLOAD, AAD)
        .await
        .unwrap();
    let b = sign_claims(&signer, TYP, &all_claims(), PAYLOAD, AAD)
        .await
        .unwrap();
    assert_eq!(a, b);
    // Pinned so a change to the wire shape is a visible diff. The same bytes
    // are rebuilt independently with `coset` in `claims_interop_coset.rs`.
    assert_eq!(common::hex(&a), PINNED_ED25519_HEX);
}

const PINNED_ED25519_HEX: &str = "d284585fa401320448be5de2f4bcdc383a0fa403647661616d041a6ab14990061a6ab13b800750a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a51078296170706c69636174696f6e2f766e642e6372617465737461636b2e746573742d6772616e742b637774a04e6772616e743a20352073686565705840d6914e8009b2f5b5daabaa49f7b501b6fadc71a77f1cb2b15c172f0685cff8afd8d0f706cc442293e8bc92c32b2b004e51f8540637ca7545a4026bf6ccd7d60b";

#[tokio::test]
async fn esp256_signatures_are_low_s() {
    let signer = common::p256();
    let bytes = sign_claims(&signer, TYP, &all_claims(), PAYLOAD, AAD)
        .await
        .unwrap();
    let range = common::forge::layout(&bytes).signature;
    let signature = p256::ecdsa::Signature::from_slice(&bytes[range]).unwrap();
    assert_eq!(signature.normalize_s(), signature);
}

/// A signer that only implements `sign`, as a KMS would (so the async
/// path and the contiguous structure are used), optionally returning the
/// high-`s` twin of an ESP256 signature.
struct Remote<S>(S, bool);

#[async_trait::async_trait]
impl<S: CoseSigner> CoseSigner for Remote<S> {
    fn alg(&self) -> CoseAlg {
        self.0.alg()
    }
    fn kid(&self) -> &[u8] {
        self.0.kid()
    }
    async fn sign(&self, to_be_signed: &[u8]) -> Result<Vec<u8>, CratestackError> {
        let signature = self.0.sign(to_be_signed).await?;
        if !self.1 {
            return Ok(signature);
        }
        let parsed = p256::ecdsa::Signature::from_slice(&signature).unwrap();
        let (r, s) = parsed.split_scalars();
        let (r, s) = (r.to_bytes(), (-*s).to_bytes());
        let high = p256::ecdsa::Signature::from_scalars(r, s).unwrap();
        // Whichever of the pair is high.
        let pick = if high.normalize_s() == high {
            parsed
        } else {
            high
        };
        Ok(pick.to_bytes().to_vec())
    }
}

#[tokio::test]
async fn a_remote_signer_produces_the_same_bytes_as_the_in_process_one() {
    let direct = sign_claims(&common::ed25519(), TYP, &all_claims(), PAYLOAD, AAD)
        .await
        .unwrap();
    let remote = sign_claims(
        &Remote(common::ed25519(), false),
        TYP,
        &all_claims(),
        PAYLOAD,
        AAD,
    )
    .await
    .unwrap();
    assert_eq!(direct, remote);
}

#[tokio::test]
async fn a_high_s_signature_from_a_remote_signer_is_normalised() {
    let remote = Remote(common::p256(), true);
    let bytes = sign_claims(&remote, TYP, &all_claims(), PAYLOAD, AAD)
        .await
        .unwrap();
    let range = common::forge::layout(&bytes).signature;
    let signature = p256::ecdsa::Signature::from_slice(&bytes[range]).unwrap();
    assert_eq!(signature.normalize_s(), signature, "normalised");
    open_claims(common::resolver().as_ref(), &bytes, TYP, AAD)
        .await
        .expect("opens");
}

#[tokio::test]
async fn freshness_and_replay_are_not_checked() {
    let signer = common::ed25519();
    let resolver = common::resolver();
    // Long expired, and issued in the far future: both open, repeatedly.
    let claims = CwtClaims {
        exp: Some(1),
        iat: Some(u64::from(u32::MAX) * 100),
        ..all_claims()
    };
    let bytes = sign_claims(&signer, TYP, &claims, PAYLOAD, AAD)
        .await
        .unwrap();
    for _ in 0..3 {
        let opened = open_claims(resolver.as_ref(), &bytes, TYP, AAD)
            .await
            .expect("opens");
        assert_eq!(
            (opened.claims.exp, opened.claims.iat),
            (claims.exp, claims.iat)
        );
    }
}

#[tokio::test]
async fn a_mismatch_of_type_aad_payload_or_key_is_refused() {
    let resolver = common::resolver();
    let bytes = sign_claims(&common::ed25519(), TYP, &all_claims(), PAYLOAD, AAD)
        .await
        .unwrap();
    let open = |bytes: Vec<u8>, typ: &'static str, aad: &'static [u8]| {
        let resolver = resolver.clone();
        async move { open_claims(resolver.as_ref(), &bytes, typ, aad).await }
    };
    assert!(matches!(
        open(bytes.clone(), "other", AAD).await,
        Err(ClaimsError::WrongType)
    ));
    assert!(matches!(
        open(bytes.clone(), "", AAD).await,
        Err(ClaimsError::WrongType)
    ));
    assert!(matches!(
        open(bytes.clone(), TYP, b"tenant-2|grant").await,
        Err(ClaimsError::Unverified)
    ));
    assert!(matches!(
        open(bytes.clone(), TYP, b"").await,
        Err(ClaimsError::Unverified)
    ));
    let layout = common::forge::layout(&bytes);
    let mut tampered = bytes.clone();
    tampered[layout.payload.start] ^= 1;
    assert!(matches!(
        open(tampered, TYP, AAD).await,
        Err(ClaimsError::Unverified)
    ));

    // Signed by a key the resolver does not know; and by a known kid's
    // algorithm twin (an ESP256 signature cannot verify as Ed25519).
    let stranger = Ed25519Signer::from_seed(&common::OTHER_ED25519_SEED);
    let foreign = sign_claims(&stranger, TYP, &all_claims(), PAYLOAD, AAD)
        .await
        .unwrap();
    assert!(matches!(
        open(foreign, TYP, AAD).await,
        Err(ClaimsError::Unverified)
    ));
}

#[tokio::test]
async fn a_resolver_that_over_returns_cannot_let_one_key_sign_for_another() {
    let stranger = Ed25519Signer::from_seed(&common::OTHER_ED25519_SEED);
    let bytes = sign_claims(&stranger, TYP, &all_claims(), PAYLOAD, AAD)
        .await
        .unwrap();
    // The resolver hands back the *wrong* key (and ours) for the stranger's kid.
    let resolver = FixedResolver(vec![
        common::ed25519().verify_key(),
        common::p256().verify_key(),
    ]);
    assert!(matches!(
        open_claims(&resolver, &bytes, TYP, AAD).await,
        Err(ClaimsError::Unverified)
    ));
    // Given the right key among the junk, it opens, and names that key.
    let resolver = FixedResolver(vec![common::p256().verify_key(), stranger.verify_key()]);
    let opened = open_claims(&resolver, &bytes, TYP, AAD)
        .await
        .expect("opens");
    assert_eq!(opened.key_thumbprint, stranger.verify_key().thumbprint());
}

#[tokio::test]
async fn a_resolver_outage_is_not_a_verdict() {
    let bytes = sign_claims(&common::ed25519(), TYP, &all_claims(), PAYLOAD, AAD)
        .await
        .unwrap();
    let error = open_claims(&FailingResolver, &bytes, TYP, AAD)
        .await
        .unwrap_err();
    assert!(matches!(error, ClaimsError::Resolver(_)), "{error:?}");
    assert!(matches!(error.into_error(), CratestackError::Internal(_)));
    // An empty answer for an unknown kid is `Ok(vec![])`: a verdict.
    let empty = StaticVerifierResolver::new();
    assert!(matches!(
        open_claims(&empty, &bytes, TYP, AAD).await,
        Err(ClaimsError::Unverified)
    ));
}

#[test]
fn errors_map_onto_the_envelopes_401_and_500() {
    for verdict in [
        ClaimsError::Malformed,
        ClaimsError::WrongType,
        ClaimsError::Unverified,
    ] {
        match verdict.into_error() {
            CratestackError::Unauthorized(message) => assert_eq!(message, UNAUTHENTICATED),
            other => panic!("{other:?}"),
        }
    }
    let misuse = ClaimsError::Misuse("x").into_error();
    assert!(matches!(misuse, CratestackError::Internal(_)));
}

#[tokio::test]
async fn bad_inputs_are_misuse_not_panics() {
    let signer = common::ed25519();
    let misuse = |result: Result<Vec<u8>, ClaimsError>| match result {
        Err(ClaimsError::Misuse(_)) => {}
        other => panic!("expected misuse, got {other:?}"),
    };
    // HMAC / Mac0 is refused.
    for alg in [CoseAlg::Hmac256_64, CoseAlg::Hmac256_256] {
        misuse(sign_claims(&common::hmac(alg), TYP, &all_claims(), PAYLOAD, AAD).await);
    }
    misuse(sign_claims(&signer, "", &all_claims(), PAYLOAD, AAD).await);
    misuse(
        sign_claims(
            &signer,
            &"t".repeat(MAX_TYP_LEN + 1),
            &all_claims(),
            PAYLOAD,
            AAD,
        )
        .await,
    );
    for aud in ["".to_owned(), "a".repeat(MAX_AUD_LEN + 1)] {
        let claims = CwtClaims {
            aud: Some(aud),
            ..CwtClaims::default()
        };
        misuse(sign_claims(&signer, TYP, &claims, PAYLOAD, AAD).await);
    }
    for cti in [vec![], vec![1; MAX_CTI_LEN + 1]] {
        let claims = CwtClaims {
            cti: Some(cti),
            ..CwtClaims::default()
        };
        misuse(sign_claims(&signer, TYP, &claims, PAYLOAD, AAD).await);
    }
}

#[tokio::test]
async fn the_documented_bounds_are_inclusive() {
    let claims = CwtClaims {
        aud: Some("a".repeat(MAX_AUD_LEN)),
        cti: Some(vec![9; MAX_CTI_LEN]),
        ..CwtClaims::default()
    };
    let typ = "t".repeat(MAX_TYP_LEN);
    let bytes = sign_claims(&common::ed25519(), &typ, &claims, PAYLOAD, AAD)
        .await
        .unwrap();
    let opened = open_claims(common::resolver().as_ref(), &bytes, &typ, AAD)
        .await
        .unwrap();
    assert_eq!(opened.claims, claims);
}

/// Signers that misbehave: the object is refused before it can exist.
struct Misbehaving {
    inner: Ed25519Signer,
    kid: Vec<u8>,
    truncate: bool,
    fail: bool,
}

#[async_trait::async_trait]
impl CoseSigner for Misbehaving {
    fn alg(&self) -> CoseAlg {
        CoseAlg::Ed25519
    }
    fn kid(&self) -> &[u8] {
        &self.kid
    }
    async fn sign(&self, to_be_signed: &[u8]) -> Result<Vec<u8>, CratestackError> {
        if self.fail {
            return Err(CratestackError::Unavailable("kms down".to_owned()));
        }
        let mut signature = self.inner.sign(to_be_signed).await?;
        if self.truncate {
            signature.pop();
        }
        Ok(signature)
    }
}

#[tokio::test]
async fn a_misbehaving_signer_is_refused() {
    let base = || Misbehaving {
        inner: common::ed25519(),
        kid: common::ed25519().kid().to_vec(),
        truncate: false,
        fail: false,
    };
    let run = |signer: Misbehaving| async move {
        sign_claims(&signer, TYP, &all_claims(), PAYLOAD, AAD).await
    };
    assert!(run(base()).await.is_ok());
    assert!(matches!(
        run(Misbehaving {
            kid: vec![1; 7],
            ..base()
        })
        .await,
        Err(ClaimsError::Misuse(_))
    ));
    assert!(matches!(
        run(Misbehaving {
            truncate: true,
            ..base()
        })
        .await,
        Err(ClaimsError::Misuse(_))
    ));
    assert!(matches!(
        run(Misbehaving {
            fail: true,
            ..base()
        })
        .await,
        Err(ClaimsError::Signer(_))
    ));
}

#[tokio::test]
async fn debug_does_not_print_the_payload() {
    let bytes = sign_claims(&common::ed25519(), TYP, &all_claims(), PAYLOAD, AAD)
        .await
        .unwrap();
    let opened = open_claims(common::resolver().as_ref(), &bytes, TYP, AAD)
        .await
        .unwrap();
    let shown = format!("{opened:?}");
    assert!(shown.contains("payload_len"), "{shown}");
    assert!(!shown.contains("sheep"), "{shown}");
}
