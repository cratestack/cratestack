//! Minting id_jwts: plain signed JWTs ([`issue_id_token`]) and SD-JWTs with
//! selectively-disclosable claims ([`issue_sd_id_token`]), plus the default
//! claim-set builder used by issuers and test fixtures alike.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer, SigningKey};

use super::{
    claims::{
        DEFAULT_ID_TOKEN_AUDIENCE, DisclosureClaim, IdTokenClaims, IdTokenClaimsParams,
        IssuedSdIdToken, JwtHeader, default_role,
    },
    disclosure::disclosure_digest,
};
use crate::{AuthError, Confirmation};

pub fn issue_id_token(
    signing_key: &SigningKey,
    issuer_kid: &str,
    claims: &IdTokenClaims,
) -> Result<String, AuthError> {
    let header = JwtHeader {
        alg: "EdDSA".to_string(),
        typ: "JWT".to_string(),
        kid: issuer_kid.to_string(),
    };
    let encoded_header = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&header)
            .map_err(|error| AuthError::IdTokenEncoding(error.to_string()))?,
    );
    let encoded_claims = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(claims)
            .map_err(|error| AuthError::IdTokenEncoding(error.to_string()))?,
    );
    let signing_input = format!("{encoded_header}.{encoded_claims}");
    let signature = signing_key.sign(signing_input.as_bytes());
    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    ))
}

/// Issue an SD-JWT: a regular JWT whose `_sd[]` array carries digests of the supplied
/// disclosures, plus the disclosure strings appended in the compact form
/// `<jwt>~<disclosure1>~...~`. Holders forward selected disclosures to verifiers via
/// the `id_jwt` Authorization parameter; verifiers recompute digests and look them up
/// in `_sd[]` to recover claim values.
pub fn issue_sd_id_token(
    signing_key: &SigningKey,
    issuer_kid: &str,
    base_claims: &IdTokenClaims,
    disclosures: &[DisclosureClaim],
) -> Result<IssuedSdIdToken, AuthError> {
    let mut claims = base_claims.clone();
    let mut disclosure_strings: Vec<String> = Vec::with_capacity(disclosures.len());
    let mut digests: Vec<String> = claims.sd.clone();
    for disclosure in disclosures {
        let salt = cuid2::create_id();
        let array = serde_json::to_vec(&serde_json::json!([
            salt,
            disclosure.name,
            disclosure.value
        ]))
        .map_err(|error| AuthError::IdTokenEncoding(error.to_string()))?;
        let encoded = URL_SAFE_NO_PAD.encode(array);
        digests.push(disclosure_digest(&encoded));
        disclosure_strings.push(encoded);
    }
    if !disclosures.is_empty() {
        claims.sd = digests;
        claims.sd_alg.get_or_insert_with(|| "sha-256".to_owned());
    }

    let jwt = issue_id_token(signing_key, issuer_kid, &claims)?;
    let mut compact = jwt.clone();
    for disclosure in &disclosure_strings {
        compact.push('~');
        compact.push_str(disclosure);
    }
    if !disclosure_strings.is_empty() {
        compact.push('~');
    }

    Ok(IssuedSdIdToken {
        compact,
        jwt,
        disclosures: disclosure_strings,
    })
}

pub fn default_id_token_claims(params: IdTokenClaimsParams<'_>) -> IdTokenClaims {
    let iat = chrono::Utc::now();
    // Placeholder `exp` only. The real issuer OVERWRITES `claims.exp` with
    // the policy TTL (short-lived, refreshed) before signing. This long
    // default exists so synthetic tokens minted directly in tests/harnesses
    // (which never go through a real issuer) stay valid for the duration of
    // a test run.
    let exp = iat + chrono::Duration::days(365);
    IdTokenClaims {
        iss: params.issuer.to_string(),
        sub: params.subject.to_string(),
        aud: DEFAULT_ID_TOKEN_AUDIENCE.to_string(),
        azp: params.client_id.to_string(),
        iat: iat.timestamp(),
        exp: exp.timestamp(),
        // No `jti` by default; a real issuer stamps a fresh one per mint.
        // Synthetic tokens built directly in tests don't need it.
        jti: None,
        cnf: Confirmation {
            kid: params.bound_key_id.to_string(),
            jwk: params.bound_key_jwk,
        },
        main_email: params.main_email,
        main_phone: params.main_phone,
        main_address: params.main_address,
        profile_version: params.profile_version,
        enrollment_status: params.enrollment_status.to_string(),
        kyc_status: params.kyc_status,
        // Default to the non-privileged role. A real issuer overwrites this
        // with the server-derived role; every other construction site
        // (tests, fixtures) gets a plain user token.
        role: default_role(),
        sd: Vec::new(),
        sd_alg: None,
    }
}

/// Returns the issuer's view of the disclosures from `params`. Useful when callers want
/// to construct claims independently from the disclosure list (`default_id_token_claims`
/// always returns an empty `_sd[]` because the digests are filled in by
/// `issue_sd_id_token`).
pub fn take_disclosures(params: IdTokenClaimsParams<'_>) -> (IdTokenClaims, Vec<DisclosureClaim>) {
    let disclosures = params.disclosures;
    let claims = default_id_token_claims(IdTokenClaimsParams {
        disclosures: Vec::new(),
        ..params
    });
    (claims, disclosures)
}
