use super::{challenge, challenge_expiry};
use crate::ID_TOKEN_GRANT;

#[test]
fn issues_cuid_values() {
    let challenge = challenge();
    assert!(challenge.starts_with('c'));
    assert!(
        challenge
            .chars()
            .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit())
    );
    assert!(challenge_expiry() > chrono::Utc::now());
    assert!(ID_TOKEN_GRANT.contains("id-sd-jwt"));
}
