//! Reqwest-backed [`DataSource`] for deployed CrateStack services.
//!
//! Phase 1a only exposes the constructor + auth-header resolution so
//! [`crate::workspace::LoadedWorkspace`] can register API targets at
//! boot. The `list` and `get` paths currently return
//! [`DataError::Unsupported`]; Phase 1b wires them to the service's
//! RPC surface.

use std::sync::Arc;

use async_trait::async_trait;
use cratestack_core::Schema;
use reqwest::Client;

use super::{DataError, DataSource, Page, PageRequest, Row};
use crate::config::ApiAuth;

#[derive(Debug, Clone)]
pub struct ApiSource {
    #[allow(dead_code)] // Used in Phase 1b when list/get hit the upstream.
    client: Client,
    #[allow(dead_code)]
    base_url: String,
    #[allow(dead_code)]
    auth_header: Option<(String, String)>,
    #[allow(dead_code)]
    schema: Arc<Schema>,
}

impl ApiSource {
    pub fn new(
        base_url: String,
        auth: Option<&ApiAuth>,
        schema: Arc<Schema>,
    ) -> Result<Self, reqwest::Error> {
        let client = Client::builder().build()?;
        let auth_header = auth.map(|a| match a {
            ApiAuth::Bearer { token } => {
                ("Authorization".to_owned(), format!("Bearer {token}"))
            }
            ApiAuth::Header { name, value } => (name.clone(), value.clone()),
        });
        Ok(Self {
            client,
            base_url,
            auth_header,
            schema,
        })
    }
}

#[async_trait]
impl DataSource for ApiSource {
    async fn list(&self, _model: &str, _page: PageRequest<'_>) -> Result<Page, DataError> {
        Err(DataError::Unsupported {
            what: "API-backed list (lands in Phase 1b)",
        })
    }

    async fn get(&self, _model: &str, _pk: &str) -> Result<Option<Row>, DataError> {
        Err(DataError::Unsupported {
            what: "API-backed get (lands in Phase 1b)",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cratestack_core::Schema;

    fn empty_schema() -> Arc<Schema> {
        Arc::new(
            cratestack_parser::parse_schema(
                "model Probe {\n  id String @id\n}\n",
            )
            .expect("trivial schema parses"),
        )
    }

    #[test]
    fn builds_bearer_header() {
        let source = ApiSource::new(
            "https://example.test".to_owned(),
            Some(&ApiAuth::Bearer {
                token: "abc".to_owned(),
            }),
            empty_schema(),
        )
        .expect("client builds");
        assert_eq!(
            source.auth_header,
            Some(("Authorization".to_owned(), "Bearer abc".to_owned()))
        );
    }

    #[test]
    fn builds_custom_header() {
        let source = ApiSource::new(
            "https://example.test".to_owned(),
            Some(&ApiAuth::Header {
                name: "X-Api-Key".to_owned(),
                value: "k123".to_owned(),
            }),
            empty_schema(),
        )
        .expect("client builds");
        assert_eq!(
            source.auth_header,
            Some(("X-Api-Key".to_owned(), "k123".to_owned()))
        );
    }

    #[tokio::test]
    async fn list_and_get_return_unsupported() {
        let source = ApiSource::new("https://x".to_owned(), None, empty_schema())
            .expect("client builds");
        assert!(matches!(
            source.list("Anything", PageRequest::default()).await,
            Err(DataError::Unsupported { .. })
        ));
        assert!(matches!(
            source.get("Anything", "1").await,
            Err(DataError::Unsupported { .. })
        ));
    }
}
