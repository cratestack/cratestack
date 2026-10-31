//! The client's credential: a device key that signs every request, and the
//! id token the request carries (`Authorization: Signature ..., id_jwt=...`,
//! the in-house scheme `SignedRequestAuthProvider` verifies).
//!
//! The generated client calls [`Device`] through `RequestAuthorizer` for
//! every request, REST or RPC, so swapping the token after a re-proof is
//! all a retry needs: the next send carries the new one.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{SecondsFormat, Utc};
use cratestack::axum::http::Method;
use cratestack_auth::{SignRequestParams, content_sha256_base64url, sign_request};
use cratestack_client_rust::{AuthorizationRequest, ClientError, RequestAuthorizer};
use ed25519_dalek::{SigningKey, VerifyingKey};

/// A device key and the id token bound to it.
pub struct Device {
    key_id: String,
    key: SigningKey,
    token: Mutex<String>,
    nonces: AtomicU64,
}

impl Device {
    /// A device with a fixed key, so every run is the same; a real device
    /// keeps its key in the secure element. It holds no token until
    /// [`Device::use_token`].
    pub fn new(key_id: &str) -> Self {
        Self {
            key_id: key_id.to_owned(),
            key: SigningKey::from_bytes(&[0x1d; 32]),
            token: Mutex::new(String::new()),
            nonces: AtomicU64::new(0),
        }
    }

    /// The key's id: the signature's `keyId` and the token's `cnf.kid`.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// The public half, for the token's `cnf.jwk`.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.key.verifying_key()
    }

    /// Carry `token` on every request from now on.
    pub fn use_token(&self, token: String) {
        *self
            .token
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = token;
    }

    fn token(&self) -> String {
        self.token
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn nonce(&self) -> String {
        let count = self.nonces.fetch_add(1, Ordering::Relaxed);
        let start = Utc::now().timestamp_nanos_opt().unwrap_or_default();
        format!("{start:x}-{count}")
    }

    /// The `Authorization` value for one request.
    pub fn authorization(
        &self,
        method: &str,
        path: &str,
        query: Option<&str>,
        body: &[u8],
    ) -> String {
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let nonce = self.nonce();
        // The client only ever sends a valid method; POST is every call here.
        let method = Method::from_bytes(method.as_bytes()).unwrap_or(Method::POST);
        let signature = sign_request(SignRequestParams {
            signing_key: &self.key,
            method: &method,
            path,
            query,
            body,
            timestamp: &timestamp,
            nonce: &nonce,
            key_id: &self.key_id,
        });
        format!(
            "Signature keyId=\"{}\", timestamp=\"{timestamp}\", nonce=\"{nonce}\", \
             signature=\"{signature}\", alg=\"Ed25519\", content_sha256=\"{}\", id_jwt=\"{}\"",
            self.key_id,
            content_sha256_base64url(body),
            self.token(),
        )
    }
}

#[async_trait::async_trait]
impl RequestAuthorizer for Device {
    async fn authorize(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError> {
        let authorization = self.authorization(
            &request.method,
            &request.path,
            request.canonical_query.as_deref(),
            &request.body,
        );
        Ok(vec![("authorization".to_owned(), authorization)])
    }
}
