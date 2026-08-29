//! Regression coverage for cratestack#666's remaining half: `field in
//! [A, B]` / `field not in [A, B]` in a `@@allow`/`@@deny` read policy.
//!
//! The decisive assertion is the same one
//! `policy_db_enum_literal.rs` makes for equality — that the generated
//! SQL predicate actually restricts rows, not merely that the schema
//! compiles. A membership policy that compiles but degrades to
//! always-true is a data leak, and against the issue's motivating
//! `Asset.purpose` shape it is specifically a KYC-document leak.
//!
//! What this file adds beyond the equality test: the interaction of
//! `in` (allow) and `not in` (deny) on the same column, and a check
//! that the emitted SQL is one flat `IN (...)` rather than the nested
//! `Or` tree the `== A || == B` workaround produces.

use cratestack::CratestackContext;
use cratestack::include_server_schema;

include_server_schema!("tests/fixtures/enum_in_list_policy.cstack", db = Postgres);

mod support;

use support::pg;

#[tokio::test]
async fn enum_in_list_policy_filters_by_variant_set() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;

    cratestack::sqlx::query("DROP TABLE IF EXISTS enum_in_list_assets")
        .execute(pool)
        .await
        .expect("asset table should reset");
    cratestack::sqlx::query(
        "CREATE TABLE enum_in_list_assets (id BIGINT PRIMARY KEY, purpose TEXT NOT NULL, label TEXT NOT NULL)",
    )
    .execute(pool)
    .await
    .expect("asset table should exist");
    cratestack::sqlx::query(
        "INSERT INTO enum_in_list_assets (id, purpose, label) VALUES \
         (1, 'product_image', 'Hero shot'), \
         (2, 'product_thumbnail', 'Thumb'), \
         (3, 'kyc_document_front', 'ID front'), \
         (4, 'kyc_selfie', 'Selfie')",
    )
    .execute(pool)
    .await
    .expect("assets should seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    // The policy gates on `purpose` alone, never on `auth()`, so an
    // anonymous caller is the sharpest test: anything visible here is
    // visible to the whole internet.
    let anonymous = CratestackContext::anonymous();

    let visible = cool
        .enum_in_list_asset()
        .find_many()
        .order_by(cratestack_schema::enum_in_list_asset::id().asc())
        .run(&anonymous)
        .await
        .expect("read of publicly-visible purposes should succeed");

    // Rows 1 and 2 satisfy the allow list AND survive the deny list.
    // Row 3 (`kyc_document_front`) fails the allow `in`. Row 4
    // (`kyc_selfie`) passes the allow `in` and is then removed by the
    // deny `not in` — which is the half that would go unnoticed if
    // `FieldNotInLiterals` were rendered wrong, since row 3 is excluded
    // either way.
    assert_eq!(
        visible.iter().map(|asset| asset.id).collect::<Vec<_>>(),
        vec![1, 2],
        "kyc_selfie (4) must be removed by the `not in` deny clause, and \
         kyc_document_front (3) by the `in` allow clause"
    );

    // Detail path, same predicate: a row excluded only by the deny
    // clause must not be reachable by primary key either.
    let denied = cool
        .enum_in_list_asset()
        .find_unique(4_i64)
        .run(&anonymous)
        .await
        .expect("scoped find_unique should succeed");
    assert!(
        denied.is_none(),
        "kyc_selfie is allowed by `in` and denied by `not in`; deny must win on the detail path too"
    );

    let excluded = cool
        .enum_in_list_asset()
        .find_unique(3_i64)
        .run(&anonymous)
        .await
        .expect("scoped find_unique should succeed");
    assert!(excluded.is_none());

    // All four rows exist — the filtering above is the policy's doing,
    // not a short table or an unrelated limit.
    let count: (i64,) = cratestack::sqlx::query_as("SELECT COUNT(*) FROM enum_in_list_assets")
        .fetch_one(pool)
        .await
        .expect("row count should query");
    assert_eq!(count.0, 4);
}

/// The shape check, separate from the filtering check: `in` must emit
/// one flat `IN (...)` with a bind slot per element. The `== A || == B`
/// spelling this replaces renders as a nested `OR` of single
/// comparisons, so asserting on `IN (` is what distinguishes the new
/// predicate from a desugaring to the old one.
///
/// Runs without a database — `preview_scoped_sql` renders from the
/// compiled policy and the caller context alone.
#[tokio::test]
async fn in_list_renders_one_flat_in_clause_with_a_slot_per_element() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let cool = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let sql = cool
        .enum_in_list_asset()
        .find_unique(1_i64)
        .preview_scoped_sql(&CratestackContext::anonymous());

    assert!(
        sql.contains("purpose IN ($1, $2, $3)"),
        "expected a flat three-slot IN for the allow clause, got: {sql}"
    );
    assert!(
        sql.contains("purpose NOT IN ($4, $5)"),
        "expected the deny clause's NOT IN to continue the bind numbering at $4, got: {sql}"
    );
}
