//! The identity side: an issuer that mints the id tokens a signed request
//! carries, and publishes the key that verifies them.
//!
//! An id token says *how* and *when* the user last authenticated: `amr`
//! (RFC 8176 methods) and `auth_time`. The server's provider copies both onto
//! the request's principal, and `@authn` reads them there (ADR 0022). Running
//! the re-authentication ceremony itself is the auth library's job, not
//! CrateStack's (cratestack#1244, out of scope), so [`Issuer::reprove`]
//! stands in for it: in a real deployment it is the auth service answering a
//! fresh proof, such as a signature by the device's user-present key or a
//! passkey assertion.

use chrono::Utc;
use cratestack_auth::{
    AuthError, IdTokenClaimsParams, JwksDocument, default_id_token_claims, issue_id_token,
    issuer_jwk, jwks, verifying_key_jwk,
};
use ed25519_dalek::SigningKey;

/// The issuer's name, the id token's `iss`.
pub const ISSUER: &str = "https://auth.step-up.example";

const ISSUER_KID: &str = "step-up-issuer-v1";

/// How a user authenticated: the methods (RFC 8176) and how many seconds ago.
#[derive(Debug, Clone, Copy)]
pub struct Authentication {
    /// The `amr` values the token carries.
    pub methods: &'static [&'static str],
    /// How long ago, in seconds, the user authenticated.
    pub age_seconds: i64,
}

impl Authentication {
    /// A password sign-in twenty minutes ago: the session an app holds
    /// most of the time, enough for `balance`, too old and too weak for
    /// `payInvoice`.
    pub const STALE_PASSWORD: Self = Self {
        methods: &["pwd"],
        age_seconds: 20 * 60,
    };

    /// A proof made just now with the device's hardware-bound key, with the
    /// user present: what `payInvoice`'s `@authn(amr: ["hwk"])` asks for.
    pub const FRESH_HARDWARE_KEY: Self = Self {
        methods: &["hwk", "user"],
        age_seconds: 0,
    };
}

/// Mints id tokens bound to one device key.
pub struct Issuer {
    key: SigningKey,
}

impl Issuer {
    /// An issuer with a fixed key, so every run of the example is the same.
    /// A real issuer loads its key from a secret store.
    pub fn new() -> Self {
        Self {
            key: SigningKey::from_bytes(&[0x5a; 32]),
        }
    }

    /// The JWKS document the server's `IdTokenVerifier` fetches.
    pub fn jwks(&self) -> JwksDocument {
        jwks(vec![issuer_jwk(&self.key, ISSUER_KID)])
    }

    /// An id token for `subject`, bound to `device` (its `cnf` names the
    /// key every request is signed with), saying `how` the user
    /// authenticated.
    pub fn mint(
        &self,
        subject: &str,
        device: &crate::device::Device,
        how: Authentication,
    ) -> Result<String, AuthError> {
        let mut claims = default_id_token_claims(IdTokenClaimsParams {
            issuer: ISSUER,
            client_id: "step-up-example",
            subject,
            bound_key_id: device.key_id(),
            bound_key_jwk: Some(verifying_key_jwk(&device.verifying_key(), device.key_id())),
            profile_version: 1,
            enrollment_status: "active",
            kyc_status: None,
            main_email: None,
            main_phone: None,
            main_address: None,
            disclosures: Vec::new(),
        });
        claims.auth_time = Some(Utc::now().timestamp() - how.age_seconds);
        claims.amr = how
            .methods
            .iter()
            .map(|method| (*method).to_owned())
            .collect();
        issue_id_token(&self.key, ISSUER_KID, &claims)
    }

    /// The re-proof: the user authenticated again, now, with the hardware
    /// key, and the device gets a token that says so.
    pub fn reprove(&self, subject: &str, device: &crate::device::Device) -> Result<(), AuthError> {
        let token = self.mint(subject, device, Authentication::FRESH_HARDWARE_KEY)?;
        device.use_token(token);
        Ok(())
    }
}

impl Default for Issuer {
    fn default() -> Self {
        Self::new()
    }
}
