//! cratestack#1040 (MCP phase 5): read-only resources against real
//! Postgres, with REST as the oracle for visibility. What only a database
//! can decide, and so what this proves:
//!
//! - a record the caller may read comes back shaped exactly like REST's
//!   `GET /<plural>/{id}` body; one `@@allow("read", ...)` hides is absent
//!   over REST *and* MCP, and over MCP its error is byte-identical to a
//!   missing id's (`mcp_resources_support/parity.rs`);
//! - a collection is exactly the rows REST lists for the same caller, and
//!   exactly the rows the seed rule says that caller may read — so loosening
//!   the model's read policy fails this test, not just REST parity
//!   (`parity.rs`);
//! - page sizes follow Q3 (50 by default, 200 at most, `max_page_size: 20`
//!   lower), a tampered cursor is `-32602`, and the page SQL filters before
//!   it limits, quoted from sqlx's own statement log (`mcp_resources_support/paging.rs`);
//! - `@server_only` never reaches the output, on a model that also has a
//!   `@computed` field (`parity.rs`);
//! - odd ids, unannotated models and near-miss URIs answer exactly like a
//!   missing row; a record names no relation (its author is hidden and has
//!   a `@server_only` field); and hidden rows promise no further page
//!   (`mcp_resources_support/adversarial.rs`).
//!
//! Gated `required-features = ["mcp"]`. Run with a database and
//! `CRATESTACK_REQUIRE_DB=1` — `just test-ci-db-mcp` does — or a missing
//! database is a silent skip that still prints `ok` (CLAUDE.md, "Critical
//! test gotcha"); read `finished in` to tell.

mod mcp_resources_support;
mod support;

use mcp_resources_support::{adversarial, harness, paging, parity};

use cratestack::include_server_schema;

include_server_schema!("tests/fixtures/mcp_resources/schema.cstack", db = Postgres);

/// One test, one container, for the reason `mcp_policy_pg.rs` gives:
/// a second container start in one binary races rootless Docker's port
/// manager. Each check below seeds nothing the others depend on; they only
/// read.
#[tokio::test]
async fn mcp_resources_see_exactly_what_rest_sees() {
    support::tracing_capture::init_tracing();
    let _guard = support::pg::serial_guard().await;
    let Some((_pg, db)) = harness::seeded().await else {
        return;
    };
    parity::listings_name_segments_never_tables(&db).await;
    parity::a_record_reads_like_rest_and_a_hidden_one_like_a_missing_one(&db).await;
    parity::a_collection_is_exactly_the_rows_rest_lists(&db).await;
    paging::page_sizes_follow_q3(&db).await;
    paging::a_tampered_cursor_is_invalid_params(&db).await;
    paging::the_page_query_filters_before_it_limits(&db).await;
    adversarial::odd_uris_answer_exactly_like_a_missing_row(&db).await;
    adversarial::a_record_carries_no_relation(&db).await;
    adversarial::hidden_rows_promise_no_further_page(&db).await;
}
