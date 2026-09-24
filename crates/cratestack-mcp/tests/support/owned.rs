//! A resource table whose visibility depends on *who reads*, the one
//! property [`super::resources::FakeResources`] cannot witness: there every
//! row is hidden from every caller alike, so a read that ran as the wrong
//! context (a stored one, an anonymous one) would still pass.
//!
//! Here row `n` belongs to `u-a` when `n` is odd and to `u-b` when it is
//! even, and a caller sees only its own rows, as an
//! `@@allow("read", owner == auth().id)` policy would. A context with no
//! `id` sees nothing. Like the generated SQL, the page filters before it
//! skips and takes.

use std::sync::Arc;
use std::sync::Mutex;

use cratestack_core::{CratestackContext, CratestackError};
use cratestack_mcp::{ArgumentsError, McpTools, ResourceDescriptor, ToolDescriptor};
use serde_json::{Value, json};

use super::resources::{NoCall, RESOURCES};

/// Rows `1..=ROWS` exist in every resource.
pub const OWNED_ROWS: u64 = 6;

pub fn owner(id: u64) -> &'static str {
    if id % 2 == 1 { "u-a" } else { "u-b" }
}

/// Records the principal every read ran as (`None` for a context without
/// an `id`), so a test can assert on the context itself, not only on what
/// it happened to filter.
#[derive(Clone, Default)]
pub struct OwnedResources {
    pub readers: Arc<Mutex<Vec<Option<String>>>>,
}

impl OwnedResources {
    pub fn readers(&self) -> Vec<Option<String>> {
        self.readers.lock().unwrap().clone()
    }

    fn reader(&self, ctx: &CratestackContext) -> Option<String> {
        let reader = ctx.principal_actor_id().map(str::to_owned);
        self.readers.lock().unwrap().push(reader.clone());
        reader
    }
}

fn row(segment: &str, id: u64) -> Value {
    json!({ "id": id, "owner": owner(id), "segment": segment })
}

impl McpTools for OwnedResources {
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
        ctx: &CratestackContext,
    ) -> Result<Option<Value>, CratestackError> {
        let reader = self.reader(ctx);
        let Ok(id) = id.parse::<u64>() else {
            return Ok(None);
        };
        let visible = (1..=OWNED_ROWS).contains(&id) && reader.as_deref() == Some(owner(id));
        Ok(visible.then(|| row(segment, id)))
    }

    async fn read_page(
        &self,
        segment: &str,
        limit: u32,
        offset: u64,
        ctx: &CratestackContext,
    ) -> Result<Vec<Value>, CratestackError> {
        let reader = self.reader(ctx);
        Ok((1..=OWNED_ROWS)
            .filter(|id| reader.as_deref() == Some(owner(*id)))
            .skip(offset as usize)
            .take(limit as usize)
            .map(|id| row(segment, id))
            .collect())
    }
}
