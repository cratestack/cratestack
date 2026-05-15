//! Reqwest-backed [`DataSource`] for deployed CrateStack services.
//!
//! Studio talks to the service's macro-generated REST routes — the
//! same surface the TypeScript / Dart clients consume. Pagination is
//! offset/limit on the wire (matching `cratestack_core::Page<T>`);
//! Studio translates that to its cursor abstraction by encoding the
//! next offset as the opaque cursor string.
//!
//! Relation follow is intentionally out of scope for the API backend
//! — the generated REST surface doesn't expose arbitrary column
//! filters, so relation traversal needs DB access.

mod ops;
mod transport;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use cratestack_core::Schema;
use reqwest::Client;

use super::{
    ColumnSnapshot, DataError, DataSource, Page, PageRequest, PkCast, Row, SqlOp, SqlPreview,
};
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
}

#[async_trait]
impl DataSource for ApiSource {
    async fn list(&self, model: &str, page: PageRequest<'_>) -> Result<Page, DataError> {
        ops::list(&self.client, &self.base_url, &self.auth_header, model, page).await
    }

    async fn get(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError> {
        ops::get(&self.client, &self.base_url, &self.auth_header, model, pk).await
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

    async fn create(&self, model: &str, payload: &Row) -> Result<Row, DataError> {
        ops::create(&self.client, &self.base_url, &self.auth_header, model, payload).await
    }

    async fn update(
        &self,
        model: &str,
        pk: &str,
        payload: &Row,
    ) -> Result<Option<Row>, DataError> {
        ops::update(&self.client, &self.base_url, &self.auth_header, model, pk, payload).await
    }

    async fn delete(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError> {
        ops::delete(&self.client, &self.base_url, &self.auth_header, model, pk).await
    }

    async fn preview_sql(
        &self,
        _op: SqlOp,
        _model: &str,
        _pk: Option<&str>,
        _payload: Option<&Row>,
    ) -> Result<SqlPreview, DataError> {
        Err(DataError::Unsupported {
            what: "SQL preview against API targets — the upstream service runs the query, not Studio",
        })
    }

    async fn inspect_columns(
        &self,
        _model: &str,
    ) -> Result<Option<Vec<ColumnSnapshot>>, DataError> {
        Err(DataError::Unsupported {
            what: "drift inspection against API targets — Studio needs DB access for information_schema",
        })
    }
}
