//! [`ResourceDescriptor`]: one exposed model, as the generated table states
//! it at compile time.

use cratestack_core::OpDescriptor;

/// One `@@mcp(resource: "<segment>")` model.
///
/// `#[non_exhaustive]` so a later phase can add a field without a breaking
/// release; the generated code builds it with [`ResourceDescriptor::new`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ResourceDescriptor {
    /// The URI authority, `<schema>` in `cratestack://<schema>/<segment>`.
    /// The same for every resource of one table.
    pub schema: &'static str,
    /// The author's `@@mcp(resource: "...")` segment, never a table or
    /// model name (ADR 0002 security requirement 11).
    pub segment: &'static str,
    /// The largest page a collection read returns: `max_page_size:` when
    /// the model declares one, else `MCP_MAX_PAGE_SIZE` (200, Q3). The
    /// page-size rule re-clamps it to `1..=200`, so a hand-written table
    /// cannot raise the framework ceiling either.
    pub max_page_size: u32,
    /// Admission facts for a by-id read: the same `model.<Model>.get`
    /// descriptor RPC would advertise. `op_id` is diagnostic only and never
    /// sent to an agent.
    pub get_op: &'static OpDescriptor,
    /// Admission facts for a collection read (`model.<Model>.list`).
    pub list_op: &'static OpDescriptor,
}

impl ResourceDescriptor {
    pub const fn new(
        schema: &'static str,
        segment: &'static str,
        max_page_size: u32,
        get_op: &'static OpDescriptor,
        list_op: &'static OpDescriptor,
    ) -> Self {
        Self {
            schema,
            segment,
            max_page_size,
            get_op,
            list_op,
        }
    }

    /// `cratestack://<schema>/<segment>`.
    pub fn collection_uri(&self) -> String {
        format!(
            "{}://{}/{}",
            super::RESOURCE_SCHEME,
            self.schema,
            self.segment
        )
    }
}
