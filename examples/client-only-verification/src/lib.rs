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
