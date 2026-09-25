//! `StaticVerifierResolver` answers by `(kid, alg)`, as the
//! `CoseVerifierResolver` contract says.
//!
//! The opener would refuse a wrong-algorithm or wrong-`kid` candidate on
//! its own (`CoseVerifyKey::verify` checks the algorithm, the opener the
//! `kid`), so no envelope test notices a resolver that stops filtering: the
//! filter is defence in depth there. It is still this type's contract, and
//! the shared vectors' `neg-kid-of-another-key` only reaches the opener's
//! `kid` check through a resolver that does not pre-filter. So the filter
//! is pinned here, on the resolver itself (security review of
//! cratestack#1005, round 2: a mutation dropping `key.supports(alg)`
//! survived every other test).

mod common;

use cratestack_cose::{
    CoseAlg, CoseVerifierResolver, CoseVerifyKey, HmacSecret, StaticVerifierResolver,
};

fn resolver() -> StaticVerifierResolver {
    StaticVerifierResolver::new()
        .with_key(common::ed25519().verify_key())
        .with_key(common::p256().verify_key())
        .with_key(common::hmac(CoseAlg::Hmac256_64).verify_key())
        .with_key(common::hmac(CoseAlg::Hmac256_256).verify_key())
}

#[tokio::test]
async fn one_secret_under_two_algorithms_resolves_to_the_asked_one_only() {
    let resolver = resolver();
    let kid = common::hmac(CoseAlg::Hmac256_256).verify_key().kid();
    // The two HMAC keys share a kid: RFC 9679's symmetric thumbprint covers
    // the secret only.
    assert_eq!(kid, common::hmac(CoseAlg::Hmac256_64).verify_key().kid());
    for alg in [CoseAlg::Hmac256_64, CoseAlg::Hmac256_256] {
        let found = resolver.resolve(&kid, alg).await.expect("resolve");
        assert_eq!(found, vec![common::hmac(alg).verify_key()], "{alg:?}");
    }
    for alg in [CoseAlg::Ed25519, CoseAlg::Esp256] {
        assert!(
            resolver
                .resolve(&kid, alg)
                .await
                .expect("resolve")
                .is_empty()
        );
    }
}

#[tokio::test]
async fn a_kid_resolves_only_under_its_keys_algorithm() {
    let resolver = resolver();
    for key in [common::ed25519().verify_key(), common::p256().verify_key()] {
        for &alg in CoseAlg::ALL {
            let found = resolver.resolve(&key.kid(), alg).await.expect("resolve");
            let expected = if alg == key.alg() {
                vec![key.clone()]
            } else {
                Vec::new()
            };
            assert_eq!(found, expected, "{:?} asked as {alg:?}", key.alg());
        }
    }
}

#[tokio::test]
async fn an_unknown_or_truncated_kid_is_an_empty_answer() {
    let resolver = resolver();
    let kid = common::ed25519().verify_key().kid();
    for asked in [&[0; 8][..], &kid[..7], &[kid.as_slice(), &[0]].concat()] {
        assert!(
            resolver
                .resolve(asked, CoseAlg::Ed25519)
                .await
                .expect("resolve")
                .is_empty()
        );
    }
}

/// `hmac_from_secret` is `hmac` for a secret already held as an
/// `HmacSecret`, with the same algorithm check.
#[test]
fn a_verify_key_from_an_hmac_secret() {
    let secret = HmacSecret::new(common::HMAC_SECRET.to_vec()).expect("secret");
    for alg in [CoseAlg::Hmac256_64, CoseAlg::Hmac256_256] {
        assert_eq!(
            CoseVerifyKey::hmac_from_secret(alg, secret.clone()).expect("key"),
            CoseVerifyKey::hmac(alg, common::HMAC_SECRET.to_vec()).expect("key")
        );
    }
    for alg in [CoseAlg::Ed25519, CoseAlg::Esp256] {
        assert!(CoseVerifyKey::hmac_from_secret(alg, secret.clone()).is_err());
    }
}
