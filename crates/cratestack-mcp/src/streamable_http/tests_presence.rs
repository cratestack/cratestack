//! `CredentialPresence`'s parts the HTTP suites cannot observe: what is
//! stripped before `rmcp` and what is cut out of the log.

use super::*;

fn parts() -> Parts {
    let request = http::Request::builder()
        .header("x-attestation", "att-123")
        .header("x-signature", "sig-456")
        .header("x-signature", " ")
        .header("x-other", "kept")
        .body(())
        .unwrap();
    request.into_parts().0
}

fn device() -> CredentialPresence {
    CredentialPresence::provider_decides().with_credential_headers([
        HeaderName::from_static("x-attestation"),
        HeaderName::from_static("x-signature"),
    ])
}

/// The credential headers never reach `rmcp`; other headers do.
#[test]
fn credential_headers_are_stripped_and_others_kept() {
    let mut parts = parts();
    device().strip(&mut parts);
    assert!(parts.headers.get("x-attestation").is_none());
    assert!(parts.headers.get("x-signature").is_none());
    assert_eq!(parts.headers["x-other"], "kept");
}

/// The bearer token and every credential header value are redacted;
/// a blank value is not a secret (`replace("")` would match everywhere).
#[test]
fn secrets_are_the_token_and_the_non_blank_credential_values() {
    let secrets = device().secrets(&parts(), Some("tok"));
    assert_eq!(secrets, ["tok", "att-123", "sig-456"]);
    assert!(
        CredentialPresence::bearer()
            .secrets(&parts(), None)
            .is_empty()
    );
}

#[test]
fn the_default_is_the_bearer_rule() {
    let presence = CredentialPresence::default();
    assert!(presence.is_bearer());
    assert!(!presence.present(&parts(), false));
    assert!(presence.present(&parts(), true));
}
