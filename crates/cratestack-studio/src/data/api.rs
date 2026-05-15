//! Reqwest-backed [`DataSource`] for deployed CrateStack services.
//!
//! Studio talks to the service's macro-generated REST routes — the
//! same surface the TypeScript / Dart clients consume. Conventions:
//!
//! - `<base_url>/api/<plural-snake-model>` for the list endpoint
//! - `<base_url>/api/<plural-snake-model>/{id}` for find_unique
//!
//! Pagination is offset/limit on the wire (matching
//! `cratestack_core::Page<T>`); Studio translates that to its cursor
//! abstraction by encoding the next offset as the opaque cursor
//! string. Authentication is whatever the user configured under
//! `[target.api.auth]`.
//!
//! Relation follow is intentionally out of scope for the API backend
//! in Phase 1b — the generated REST surface doesn't expose arbitrary
//! column filters, so relation traversal needs DB access.

use std::sync::Arc;

use async_trait::async_trait;
use cratestack_core::Schema;
use cratestack_migrate::table_name;
use reqwest::header::HeaderName;
use reqwest::{Client, StatusCode};

use super::{DataError, DataSource, Page, PageRequest, PkCast, Row};
use crate::config::ApiAuth;

#[derive(Debug, Clone)]
pub struct ApiSource {
    client: Client,
    base_url: String,
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

    fn list_url(&self, model: &str) -> String {
        let plural = table_name(model);
        let trimmed = self.base_url.trim_end_matches('/');
        format!("{trimmed}/api/{plural}")
    }

    fn detail_url(&self, model: &str, pk: &str) -> String {
        format!(
            "{}/{}",
            self.list_url(model),
            urlencoding_encode(pk)
        )
    }

    fn apply_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some((name, value)) = &self.auth_header {
            let header_name: Result<HeaderName, _> = name.parse();
            if let Ok(parsed) = header_name {
                return builder.header(parsed, value);
            }
        }
        builder
    }
}

/// Tiny percent-encoder for the PK segment. Reqwest's URL builder
/// would handle this, but we're building the URL by hand so we can
/// keep the `/api/{plural}/{pk}` shape readable in logs.
fn urlencoding_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}

fn cursor_to_offset(cursor: Option<&str>) -> i64 {
    cursor.and_then(|c| c.parse().ok()).unwrap_or(0)
}

#[async_trait]
impl DataSource for ApiSource {
    async fn list(&self, model: &str, page: PageRequest<'_>) -> Result<Page, DataError> {
        let url = self.list_url(model);
        let offset = cursor_to_offset(page.cursor);
        let limit = page.limit.unwrap_or(50).clamp(1, 500) as i64;

        let builder = self
            .client
            .get(&url)
            .query(&[("limit", limit.to_string()), ("offset", offset.to_string())]);
        let response = self.apply_auth(builder).send().await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Err(DataError::UnknownModel {
                model: model.to_owned(),
            });
        }
        let bytes = response.error_for_status()?.bytes().await?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| DataError::Unsupported {
                what: "upstream response was not valid JSON (is this a cratestack service?)",
            })?;

        let rows: Vec<Row> = value
            .get("items")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_object().cloned())
                    .collect()
            })
            .unwrap_or_default();

        let has_next = value
            .get("pageInfo")
            .and_then(|p| p.get("hasNextPage"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false);

        let next_cursor = if has_next {
            Some((offset + rows.len() as i64).to_string())
        } else {
            None
        };

        Ok(Page { rows, next_cursor })
    }

    async fn get(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError> {
        let url = self.detail_url(model, pk);
        let response = self.apply_auth(self.client.get(&url)).send().await?;

        match response.status() {
            StatusCode::NOT_FOUND => Ok(None),
            status if status.is_success() => {
                let bytes = response.bytes().await?;
                let value: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(|_| DataError::Unsupported {
                        what: "upstream response was not valid JSON (is this a cratestack service?)",
                    })?;
                Ok(match value {
                    serde_json::Value::Object(map) => Some(map),
                    _ => None,
                })
            }
            _ => Err(DataError::Api(response.error_for_status().unwrap_err())),
        }
    }

    async fn follow(
        &self,
        _target_model: &str,
        _filter_column: &str,
        _filter_cast: PkCast,
        _filter_value: &str,
        _page: PageRequest<'_>,
    ) -> Result<Page, DataError> {
        Err(DataError::Unsupported {
            what: "relation follow against API targets — Studio needs DB access for arbitrary column filters",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn list_url_pluralizes_and_snake_cases_model() {
        let source = ApiSource::new(
            "https://example.test".to_owned(),
            None,
            empty_schema(),
        )
        .expect("client builds");
        assert_eq!(
            source.list_url("OrderItem"),
            "https://example.test/api/order_items"
        );
    }

    #[test]
    fn list_url_strips_trailing_slash() {
        let source = ApiSource::new(
            "https://example.test/".to_owned(),
            None,
            empty_schema(),
        )
        .expect("client builds");
        assert_eq!(source.list_url("Post"), "https://example.test/api/posts");
    }

    #[test]
    fn detail_url_percent_encodes_pk_segment() {
        let source = ApiSource::new(
            "https://example.test".to_owned(),
            None,
            empty_schema(),
        )
        .expect("client builds");
        assert_eq!(
            source.detail_url("Post", "abc/123 xy"),
            "https://example.test/api/posts/abc%2F123%20xy"
        );
    }

    #[tokio::test]
    async fn follow_returns_unsupported() {
        let source = ApiSource::new("https://x".to_owned(), None, empty_schema())
            .expect("client builds");
        let error = source
            .follow("M", "c", PkCast::Text, "v", PageRequest::default())
            .await
            .expect_err("unsupported");
        assert!(matches!(error, DataError::Unsupported { .. }));
    }
}
