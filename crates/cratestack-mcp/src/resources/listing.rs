//! `resources/list` and `resources/templates/list`.
//!
//! Both return the whole generated table in one page, unfiltered by the
//! caller's authorization (as `tools/list` is, ADR 0002 § Tools): an entry
//! names a *kind* of resource the schema author chose to expose, never a
//! row. Rows are only ever named by a caller who already has their id.
//!
//! `ttlMs: 0` with a private scope, like `tools/list`: the table is static,
//! but a redeploy changes it and nothing here knows how often that is.

use rmcp::model::{CacheScope, ListResourceTemplatesResult, ListResourcesResult};
use rmcp::model::{Resource, ResourceTemplate};

use super::{JSON_MIME, RESOURCE_SCHEME, ResourceDescriptor, page};

/// One entry per resource: its collection URI.
pub(crate) fn list_resources(table: &[ResourceDescriptor]) -> ListResourcesResult {
    let resources = table
        .iter()
        .map(|resource| {
            Resource::new(resource.collection_uri(), resource.segment)
                .with_description(collection_description(resource))
                .with_mime_type(JSON_MIME)
        })
        .collect();
    ListResourcesResult::with_all_items(resources)
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private)
}

/// Two templates per resource: one record by id, and the collection with
/// its paging parameters (RFC 6570 form-style query expansion).
pub(crate) fn list_templates(table: &[ResourceDescriptor]) -> ListResourceTemplatesResult {
    let templates = table
        .iter()
        .flat_map(|resource| {
            let base = format!("{RESOURCE_SCHEME}://{}/{}", resource.name, resource.segment);
            [
                ResourceTemplate::new(format!("{base}/{{id}}"), resource.segment)
                    .with_description(format!(
                        "One `{}` record by its id, or \"resource not found\" when it does \
                         not exist or you may not read it.",
                        resource.segment
                    ))
                    .with_mime_type(JSON_MIME),
                ResourceTemplate::new(format!("{base}{{?limit,cursor}}"), resource.segment)
                    .with_description(collection_description(resource))
                    .with_mime_type(JSON_MIME),
            ]
        })
        .collect();
    ListResourceTemplatesResult::with_all_items(templates)
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private)
}

/// Framework text, not schema metadata: how to page, and the numbers that
/// apply. The model's own `///` docs are deliberately not used, for the
/// reason `ToolDescriptor::description` gives.
fn collection_description(resource: &ResourceDescriptor) -> String {
    let ceiling = page::page_size(Some(u64::MAX), resource.max_page_size);
    let default = page::page_size(None, resource.max_page_size);
    format!(
        "`{}` records you may read, in id order, as {{\"items\": [...], \"nextCursor\": \
         \"...\"}}. `limit` defaults to {default} and is capped at {ceiling}; pass \
         `nextCursor` back as `cursor` for the next page. No `nextCursor` means the last page.",
        resource.segment
    )
}
