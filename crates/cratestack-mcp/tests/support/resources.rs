//! A hand-written resource table standing in for the generated one, so the
//! protocol side (URIs, paging, cursors, errors, admission) is tested
//! without a database. The generated table's policy — rows filtered in the
//! SQL — is tested against real Postgres in `cratestack-pg`'s
//! `tests/mcp_resources_pg.rs`.
//!
//! The fake keeps that one property honestly: `read_page` filters hidden
//! rows *before* it applies offset and limit, as the SQL does, and
//! `read_record` answers a hidden row exactly as a missing one.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cratestack_core::{CratestackContext, CratestackError, OpDescriptor, OpKind};
use cratestack_mcp::{ArgumentsError, McpTools, ResourceDescriptor, ToolDescriptor};
use serde_json::{Value, json};

static GET: OpDescriptor = read_op("model.Post.get");
static LIST: OpDescriptor = read_op("model.Post.list");

const fn read_op(op_id: &'static str) -> OpDescriptor {
    OpDescriptor {
        op_id,
        kind: OpKind::Unary,
        input_ty: "",
        output_ty: "",
        idempotent_by_default: true,
        rate_limited_by_default: true,
        auth_required: false,
    }
}

pub static RESOURCES: [ResourceDescriptor; 2] = [
    ResourceDescriptor::new("blog", "posts", 200, &GET, &LIST),
    ResourceDescriptor::new("blog", "notes", 20, &GET, &LIST),
];

/// Rows `1..=ROWS` exist in both resources; every multiple of 3 is hidden
/// from every caller.
pub const ROWS: u64 = 450;

pub fn visible(id: u64) -> bool {
    id % 3 != 0
}

/// Counts reads that reached the table, the witness that a refused read
/// never ran.
#[derive(Clone, Default)]
pub struct FakeResources {
    pub reads: Arc<AtomicUsize>,
}

impl FakeResources {
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

pub enum NoCall {}

impl McpTools for FakeResources {
    type Call = NoCall;

    fn tools(&self) -> &'static [ToolDescriptor] {
        &[]
    }

    fn decode(&self, tool: &str, _arguments: Value) -> Result<NoCall, ArgumentsError> {
        Err(ArgumentsError::new(format!("no tool `{tool}`")))
    }

    async fn execute(
        &self,
        call: NoCall,
        _ctx: &CratestackContext,
    ) -> Result<Value, CratestackError> {
        match call {}
    }

    fn resources(&self) -> &'static [ResourceDescriptor] {
        &RESOURCES
    }

    async fn read_record(
        &self,
        segment: &str,
        id: &str,
        _ctx: &CratestackContext,
    ) -> Result<Option<Value>, CratestackError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let Ok(id) = id.parse::<u64>() else {
            return Ok(None);
        };
        Ok(((1..=ROWS).contains(&id) && visible(id)).then(|| row(segment, id)))
    }

    async fn read_page(
        &self,
        segment: &str,
        limit: u32,
        offset: u64,
        _ctx: &CratestackContext,
    ) -> Result<Vec<Value>, CratestackError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok((1..=ROWS)
            .filter(|id| visible(*id))
            .skip(offset as usize)
            .take(limit as usize)
            .map(|id| row(segment, id))
            .collect())
    }
}

fn row(segment: &str, id: u64) -> Value {
    json!({ "id": id, "segment": segment })
}
