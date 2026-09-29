//! cratestack#490 verification crate — see `README.md` and this crate's
//! `Cargo.toml` doc comment for why this exists and why it is deliberately
//! **not** a workspace member.

use cratestack::client_rust::{CborCodec, ClientConfig, CratestackClient};

cratestack::include_client_schema!("schema.cstack");

pub use cratestack_schema as schema;

/// `CratestackClient::new` — no `axum::Router`, no `cratestack-axum`
/// dependency in this crate's graph to have built one against even if the
/// code wanted to. That's the whole point of this crate.
pub fn build_client(base_url: url::Url) -> schema::client::Client {
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    schema::client::Client::new(runtime)
}

/// cratestack#1104 follow-up, compiled for `wasm32-unknown-unknown` by CI's
/// `facade-disjointness` job: in a browser an authorizer may hold a value
/// that is not `Send` across an `await` (the shape of a token refresh made
/// through `fetch`) and still be attached with `with_request_authorizer`.
/// Before the trait was target-split, this module did not compile there.
#[cfg(target_arch = "wasm32")]
pub mod browser_authorizer {
    use std::rc::Rc;
    use std::sync::Arc;

    use cratestack::client_rust::{
        AuthorizationRequest, ClientError, CratestackClient, RequestAuthorizer,
    };

    pub struct RefreshingAuthorizer {
        token: Rc<str>,
    }

    #[async_trait::async_trait(?Send)]
    impl RequestAuthorizer for RefreshingAuthorizer {
        async fn authorize(
            &self,
            _request: &AuthorizationRequest,
        ) -> Result<Vec<(String, String)>, ClientError> {
            let token = Rc::clone(&self.token);
            std::future::ready(()).await;
            Ok(vec![(
                "authorization".to_owned(),
                format!("Bearer {token}"),
            )])
        }
    }

    // One thread in a browser: the `Arc` only satisfies the method's type.
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn with_refreshing_authorizer(runtime: CratestackClient, token: &str) -> CratestackClient {
        runtime.with_request_authorizer(Arc::new(RefreshingAuthorizer {
            token: Rc::from(token),
        }))
    }
}

/// cratestack#1007, compiled for `wasm32-unknown-unknown` by CI's
/// `facade-disjointness` job with `--features cose`: a signing client whose
/// key is behind a callback that holds a value that is not `Send` across an
/// `await` (the shape of a WebCrypto or JS keystore call). Before
/// `CoseSigner` was target-split, this module did not compile there.
#[cfg(all(target_arch = "wasm32", feature = "cose"))]
pub mod browser_signer {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    use cratestack::client_rust::{ClientEnvelope, ClientError, CratestackClient};
    use cratestack::cose::{
        CoseEnvelope, CoseMode, CoseVerifyKey, ExternalSigner, StaticVerifierResolver,
    };

    /// Stands in for a JS keystore handle: neither `Send` nor `Sync`.
    pub struct Keystore {
        calls: Cell<u32>,
    }

    impl Keystore {
        pub fn new() -> Self {
            Self {
                calls: Cell::new(0),
            }
        }

        /// Signs `to_be_signed` (a real one awaits a `JsFuture`).
        pub async fn sign(
            &self,
            to_be_signed: Vec<u8>,
        ) -> Result<Vec<u8>, cratestack::CratestackError> {
            std::future::ready(()).await;
            self.calls.set(self.calls.get() + 1);
            Ok(to_be_signed)
        }
    }

    // One thread in a browser: the `Arc`s only satisfy the methods' types.
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn sealed_runtime(
        runtime: CratestackClient,
        public_key_sec1: &[u8],
        server_key: CoseVerifyKey,
    ) -> Result<CratestackClient, ClientError> {
        let bad = |error: cratestack::CratestackError| ClientError::BadInput(error.to_string());
        let keystore = Rc::new(Keystore::new());
        let signer = ExternalSigner::esp256(public_key_sec1, move |tbs| {
            let keystore = Rc::clone(&keystore);
            async move { keystore.sign(tbs).await }
        })
        .map_err(bad)?;
        let envelope = CoseEnvelope::client(
            CoseMode::Sign1,
            Arc::new(signer),
            Arc::new(StaticVerifierResolver::new().with_key(server_key)),
        )
        .build()
        .map_err(bad)?;
        runtime.with_envelope(ClientEnvelope::new(envelope, "payments")?)
    }
}
