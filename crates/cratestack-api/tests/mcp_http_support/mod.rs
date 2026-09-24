//! For `tests/mcp_http.rs` (cratestack#1039): the generated schema, a
//! registry whose one procedure returns the context it was called with and
//! counts its runs, and the example audience-checking provider.

pub mod token;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cratestack::include_server_schema;
use cratestack::{CratestackContext, CratestackError, Value};

include_server_schema!("tests/fixtures/mcp_http.cstack", db = None);

pub use cratestack_schema::procedures::me;

#[derive(Clone, Default)]
pub struct Registry {
    pub runs: Arc<AtomicUsize>,
}

impl Registry {
    pub fn runs(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }
}

fn claim(ctx: &CratestackContext, name: &str) -> String {
    match ctx.auth_field(name) {
        Some(Value::String(value)) => value.clone(),
        other => format!("<{other:?}>"),
    }
}

impl cratestack_schema::procedures::ProcedureRegistry for Registry {
    async fn me(
        &self,
        _db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        _args: me::Args,
        _authorized: me::Authorized,
    ) -> Result<me::Output, CratestackError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Ok(cratestack_schema::Me {
            id: claim(ctx, "id"),
            role: claim(ctx, "role"),
            tenant: claim(ctx, "tenant"),
        })
    }
}

#[derive(Clone)]
pub struct Resolver;

impl cratestack_schema::ComputedFieldResolver for Resolver {}

pub fn tools(registry: &Registry) -> cratestack_schema::mcp::McpTools<Registry, Resolver> {
    cratestack_schema::mcp::tools(
        cratestack_schema::Cratestack::builder().build(),
        registry.clone(),
        Resolver,
    )
}
