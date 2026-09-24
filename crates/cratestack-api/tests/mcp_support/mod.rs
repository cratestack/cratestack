//! Shared by `tests/mcp_tools.rs` and `tests/mcp_admission.rs`
//! (cratestack#1038): the generated schema, a registry that counts every
//! implementation it runs, a computed-field resolver, and a raw JSON-RPC
//! client over the real stdio framing.
//!
//! The run counter is the witness the decisive tests need. "The call was
//! refused" can be read off the result; "the implementation never ran"
//! can only be read off a side effect the implementation would have had.

#![allow(dead_code)] // Each test binary uses a different subset.

pub mod client;
pub mod store;

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use cratestack::include_server_schema;
use cratestack::{CratestackContext, CratestackError, Value};

include_server_schema!("tests/fixtures/mcp_tools.cstack", db = None);

pub use cratestack_schema::procedures::{badge, internal_only, touch, transfer, whoami};

#[derive(Clone, Default)]
pub struct Registry {
    pub runs: Arc<AtomicI64>,
}

impl Registry {
    pub fn runs(&self) -> i64 {
        self.runs.load(Ordering::SeqCst)
    }

    fn run(&self) -> i64 {
        self.runs.fetch_add(1, Ordering::SeqCst) + 1
    }
}

impl cratestack_schema::procedures::ProcedureRegistry for Registry {
    async fn whoami(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: whoami::Args,
        _authorized: whoami::Authorized,
    ) -> Result<whoami::Output, CratestackError> {
        Ok(cratestack_schema::Receipt {
            amount: 0,
            run: self.run(),
        })
    }

    async fn transfer(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: transfer::Args,
        _authorized: transfer::Authorized,
    ) -> Result<transfer::Output, CratestackError> {
        Ok(cratestack_schema::Receipt {
            amount: args.args.amount,
            run: self.run(),
        })
    }

    async fn touch(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: touch::Args,
        _authorized: touch::Authorized,
    ) -> Result<touch::Output, CratestackError> {
        Ok(cratestack_schema::Receipt {
            amount: args.args.amount,
            run: self.run(),
        })
    }

    async fn badge(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: badge::Args,
        _authorized: badge::Authorized,
    ) -> Result<badge::Output, CratestackError> {
        self.run();
        Ok(cratestack_schema::Badge { label: args.label })
    }

    async fn internal_only(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: internal_only::Args,
        _authorized: internal_only::Authorized,
    ) -> Result<internal_only::Output, CratestackError> {
        Ok(args.n)
    }
}

/// Resolves `Badge`'s two computed fields from the stored label and the
/// caller, so a test can tell a resolved value from a default.
#[derive(Clone)]
pub struct Resolver;

impl cratestack_schema::ComputedFieldResolver for Resolver {
    async fn resolve_badge_shout(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::Badge,
        _ctx: &CratestackContext,
    ) -> Result<String, CratestackError> {
        Ok(source.label.to_uppercase())
    }

    async fn resolve_badge_note(
        &self,
        _db: &cratestack_schema::Cratestack,
        _source: &cratestack_schema::Badge,
        ctx: &CratestackContext,
    ) -> Result<Option<String>, CratestackError> {
        Ok(ctx.principal_actor_id().map(|id| format!("for {id}")))
    }
}

pub fn tools(registry: &Registry) -> cratestack_schema::mcp::McpTools<Registry, Resolver> {
    cratestack_schema::mcp::tools(
        cratestack_schema::Cratestack::builder().build(),
        registry.clone(),
        Resolver,
    )
}

/// An authenticated caller with an `id` and a `role` claim.
pub fn caller(id: &str, role: &str) -> CratestackContext {
    CratestackContext::authenticated([
        ("id".to_owned(), Value::String(id.to_owned())),
        ("role".to_owned(), Value::String(role.to_owned())),
    ])
}
