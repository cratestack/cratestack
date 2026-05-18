//! Server-side CrateStack example: full `include_server_schema!` wiring with
//! axum, a host-owned `AuthProvider`, and a procedure registry. Connects to
//! Postgres via `sqlx::PgPool` and exposes the generated REST surface on
//! `127.0.0.1:3000`.
//!
//! ### Run it
//!
//! ```bash
//! export DATABASE_URL=postgres://cratestack:cratestack@localhost/cratestack
//! cargo run --example server_basic -p cratestack
//! ```
//!
//! Without `DATABASE_URL`, the example prints the generated route table and
//! exits — useful for verifying compilation and previewing the surface
//! without standing up a database.
//!
//! ### Try it
//!
//! ```bash
//! curl -s http://127.0.0.1:3000/Article \
//!   -H "x-auth-id: 1" \
//!   -H "accept: application/json"
//! ```

use cratestack::axum::Router;
use cratestack::include_server_schema;
use cratestack::sqlx::PgPool;
use cratestack::{AuthProvider, CoolContext, CoolError, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use std::net::SocketAddr;

include_server_schema!("examples/server_basic.cstack", db = Postgres);

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
        {
            match id.parse::<i64>() {
                Ok(id) => fields.push(("id".to_owned(), Value::Int(id))),
                Err(error) => {
                    return core::future::ready(Err(CoolError::BadRequest(error.to_string())));
                }
            }
        }
        if let Some(role) = request.headers.get("x-role").and_then(|v| v.to_str().ok()) {
            fields.push(("role".to_owned(), Value::String(role.to_owned())));
        }
        core::future::ready(Ok(if fields.is_empty() {
            CoolContext::anonymous()
        } else {
            CoolContext::authenticated(fields)
        }))
    }
}

/// Empty procedure registry — this example only exposes model CRUD routes.
/// Adding procedures would extend the schema with `procedure ...` blocks and
/// implement them on this struct.
#[derive(Clone)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {}

fn build_router(db: cratestack_schema::Cratestack) -> Router {
    cratestack_schema::axum::router(db, Procedures, JsonCodec, HeaderAuthProvider)
}

fn print_route_table() {
    println!(
        "Generated routes ({}):",
        cratestack_schema::axum::ROUTE_TRANSPORTS.len()
    );
    for route in cratestack_schema::axum::ROUTE_TRANSPORTS {
        println!("  {:<8} {}", route.method, route.path);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            println!(
                "DATABASE_URL not set. Printing generated route table and exiting.\n\
                 Set DATABASE_URL to a running Postgres instance to bind the server."
            );
            print_route_table();
            return Ok(());
        }
    };

    let pool = PgPool::connect(&database_url).await?;
    let cool = cratestack_schema::Cratestack::builder(pool).build();
    let app = build_router(cool);

    let addr: SocketAddr = "127.0.0.1:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("listening on http://{addr}");
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
        let cool = cratestack_schema::Cratestack::builder(pool).build();
        let _router = build_router(cool);
    }

    #[test]
    fn route_table_lists_article_endpoints() {
        let paths: Vec<&str> = cratestack_schema::axum::ROUTE_TRANSPORTS
            .iter()
            .map(|r| r.path)
            .collect();
        assert!(
            paths.iter().any(|p| p.contains("articles")),
            "got: {paths:?}"
        );
    }
}
