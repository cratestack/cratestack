//! How many records a collection read returns (ADR 0002 Q3, security
//! requirement 6).
//!
//! This is the only place the rule lives: the generated `read_page` takes
//! whatever `limit` it is given. A second clamp there would make this one
//! untestable — removing either would change nothing observable.

use cratestack_core::MCP_MAX_PAGE_SIZE;

/// The page size when the URI asks for none (Q3).
pub const DEFAULT_PAGE_SIZE: u32 = 50;

/// The page size for one read. `requested` is the URI's `limit`, already
/// known to be at least 1; `resource_max` is the descriptor's
/// `max_page_size`.
///
/// A request above the ceiling is **clamped, not refused** (Q3: "a client
/// asking for more gets the maximum, not an error"). The ceiling is the
/// resource's own maximum, itself held to `1..=200` here so that neither a
/// hand-written descriptor nor a future IR change can raise it past Q3's
/// hard maximum. The default is 50, or the ceiling when that is lower: a
/// `max_page_size: 20` resource answers 20 records by default, never 50.
pub(crate) fn page_size(requested: Option<u64>, resource_max: u32) -> u32 {
    let ceiling = resource_max.clamp(1, MCP_MAX_PAGE_SIZE);
    match requested {
        None => DEFAULT_PAGE_SIZE.min(ceiling),
        Some(requested) => u32::try_from(requested).map_or(ceiling, |n| n.min(ceiling)),
    }
}
