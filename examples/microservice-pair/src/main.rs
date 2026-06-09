//! A service that is simultaneously:
//!
//! - **A server**: owns the `Order` model + Postgres + an axum router
//!   (`include_server_schema!`)
//! - **A client**: talks to an upstream `Catalog` service to validate
//!   product references before persisting an `Order`
//!   (`include_client_schema!`)
//!
//! Most realistic CrateStack microservice deployments look like this.
//! Keeping the server macro and the client macro in **separate modules**
//! prevents `cratestack_schema` name collisions and makes the "owned vs
//! upstream" boundary explicit at every call site.
//!
//! ### Run
//!
//! ```bash
//! export DATABASE_URL=postgres://cratestack:cratestack@localhost/orders
//! export CATALOG_URL=http://catalog.internal:3000
//! cargo run -p microservice-pair-example
//! ```
//!
//! Without either env var, the example prints both surfaces and exits.

use cratestack::axum::Router;
use cratestack::include_server_schema;
use cratestack::sqlx::PgPool;
use cratestack::{AuthProvider, CoolContext, CoolError, RequestContext, Value};
use cratestack_client_rust::{ClientConfig, CratestackClient};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use std::net::SocketAddr;
use std::sync::Arc;
use url::Url;

// Server-owned schema — this service is the system of record for orders.
include_server_schema!("schemas/orders.cstack", db = Postgres);

// Upstream catalog schema — this service is a *consumer* of catalog.
pub mod catalog_client {
    use cratestack::include_client_schema;
    include_client_schema!("schemas/catalog.cstack");
}

/// State carried into request handlers. In a real service you'd build this
/// with a real auth provider that issues from JWTs, mTLS, etc.
#[derive(Clone)]
struct HeaderAuthProvider;

impl AuthProvider for HeaderAuthProvider {
    type Error = CoolError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        let mut fields = Vec::new();
        if let Some(id) = request
            .headers
            .get("x-auth-id")
            .and_then(|v| v.to_str().ok())
            && let Ok(id) = id.parse::<i64>()
        {
            fields.push(("id".to_owned(), Value::Int(id)));
        }
        core::future::ready(Ok(if fields.is_empty() {
            CoolContext::anonymous()
        } else {
            CoolContext::authenticated(fields)
        }))
    }
}

#[derive(Clone)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {}

/// Thin wrapper around the generated `catalog_client::cratestack_schema::client::Client`.
/// In a larger service this would also do retries, circuit breaking, and metrics.
pub struct CatalogClient {
    runtime: Arc<CratestackClient<CborCodec>>,
}

impl CatalogClient {
    pub fn new(base_url: Url) -> Self {
        let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
        Self {
            runtime: Arc::new(runtime),
        }
    }

    /// Validate that a product exists and is in stock before we accept an order.
    pub async fn product_is_available(
        &self,
        _id: i64,
    ) -> Result<bool, cratestack_client_rust::ClientError> {
        let client =
            catalog_client::cratestack_schema::client::Client::new((*self.runtime).clone());
        let products = client.products().list(&[("limit", "1")], &[]).await?;
        Ok(products.iter().any(|p| p.inStock))
    }
}

fn build_router(db: cratestack_schema::Cratestack) -> Router {
    cratestack_schema::axum::router(db, Procedures, JsonCodec, HeaderAuthProvider)
}

fn print_surfaces() {
    println!("=== orders (this service, served via include_server_schema!) ===");
    println!("  models = {:?}", cratestack_schema::MODELS);
    println!(
        "  routes = {}",
        cratestack_schema::axum::ROUTE_TRANSPORTS.len()
    );
    println!();
    println!("=== catalog (upstream, consumed via include_client_schema!) ===");
    println!("  models = {:?}", catalog_client::cratestack_schema::MODELS);
    println!();
    println!("Set DATABASE_URL and CATALOG_URL to bind the server and talk to catalog.");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL").ok();
    let catalog_url = std::env::var("CATALOG_URL").ok();

    if database_url.is_none() || catalog_url.is_none() {
        print_surfaces();
        return Ok(());
    }

    let pool = PgPool::connect(&database_url.unwrap()).await?;
    let db = cratestack_schema::Cratestack::builder(pool).build();

    // Smoke-call catalog before we start serving — fail fast if the contract
    // doesn't match.
    let catalog = CatalogClient::new(Url::parse(&catalog_url.unwrap())?);
    match catalog.product_is_available(1).await {
        Ok(available) => println!("catalog reachable; sample product available = {available}"),
        Err(error) => eprintln!("warning: catalog unreachable: {error}"),
    }

    let app = build_router(db);
    let addr: SocketAddr = "127.0.0.1:3001".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("orders-service listening on http://{addr}");
    cratestack::axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn router_builds_offline() {
        let pool = cratestack::sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://x:x@127.0.0.1/none")
            .expect("lazy pool should parse");
        let db = cratestack_schema::Cratestack::builder(pool).build();
        let _router = build_router(db);
    }

    #[test]
    fn server_and_client_modules_have_disjoint_models() {
        assert!(cratestack_schema::MODELS.contains(&"Order"));
        assert!(!cratestack_schema::MODELS.contains(&"Product"));
        assert!(catalog_client::cratestack_schema::MODELS.contains(&"Product"));
        assert!(!catalog_client::cratestack_schema::MODELS.contains(&"Order"));
    }
}
