//! Regression coverage for cratestack#666: a required enum field must
//! be usable in a literal `@@allow`/`@@deny` read-policy comparison,
//! and — the decisive assertion, not just "it compiles" — the
//! generated SQL predicate must actually restrict rows to the allowed
//! variant(s), not merely typecheck. A policy that compiles but
//! doesn't filter is a data leak (see the issue's `Asset.purpose`
//! motivating example: public marketing images vs. KYC documents in
//! the same table, discriminated only by an enum column).

use cratestack::CratestackContext;
use cratestack::include_server_schema;

include_server_schema!("tests/fixtures/enum_literal_policy.cstack", db = Postgres);

mod support;

use support::pg;

#[tokio::test]
async fn required_enum_field_literal_policy_filters_by_variant() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;

    cratestack::sqlx::query("DROP TABLE IF EXISTS enum_policy_assets")
        .execute(pool)
        .await
        .expect("enum policy asset table should reset");
    cratestack::sqlx::query(
        "CREATE TABLE enum_policy_assets (id BIGINT PRIMARY KEY, purpose TEXT NOT NULL, label TEXT NOT NULL)",
    )
    .execute(pool)
    .await
    .expect("enum policy asset table should exist");
    cratestack::sqlx::query(
        "INSERT INTO enum_policy_assets (id, purpose, label) VALUES \
         (1, 'product_image', 'Hero shot'), \
         (2, 'product_thumbnail', 'Thumb'), \
         (3, 'kyc_document_front', 'ID front'), \
         (4, 'kyc_document_front', 'ID front 2')",
    )
    .execute(pool)
    .await
    .expect("enum policy assets should seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    // Anonymous is enough — the policy under test doesn't gate on
    // `auth()` at all, only on the `purpose` enum literal.
    let anonymous = CratestackContext::anonymous();

    let visible = cool
        .enum_policy_asset()
        .find_many()
        .order_by(cratestack_schema::enum_policy_asset::id().asc())
        .run(&anonymous)
        .await
        .expect("read of publicly-visible purposes should succeed");

    // Only the two public-purpose rows (1, 2) are visible; the two
    // `kyc_document_front` rows (3, 4) must be filtered out by the SQL
    // predicate, not just excluded by chance. If the policy compiled
    // but silently degraded to an always-true predicate (the exact
    // failure mode this test guards against), all four rows would
    // come back here instead.
    assert_eq!(
        visible.iter().map(|asset| asset.id).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        visible[0].purpose,
        cratestack_schema::AssetPurpose::product_image
    );
    assert_eq!(
        visible[1].purpose,
        cratestack_schema::AssetPurpose::product_thumbnail
    );

    // `find_unique` on a hidden row must also come back empty — same
    // predicate, detail path.
    let hidden = cool
        .enum_policy_asset()
        .find_unique(3_i64)
        .run(&anonymous)
        .await
        .expect("scoped find_unique should succeed");
    assert!(hidden.is_none());

    // Sanity check the negative: a purpose not named in the policy
    // (`kyc_document_front`) really is absent from the SQL result set,
    // not merely reordered or truncated by an unrelated limit.
    assert!(
        !visible
            .iter()
            .any(|asset| asset.purpose == cratestack_schema::AssetPurpose::kyc_document_front)
    );

    let count: (i64,) = cratestack::sqlx::query_as("SELECT COUNT(*) FROM enum_policy_assets")
        .fetch_one(pool)
        .await
        .expect("row count should query");
    assert_eq!(count.0, 4, "all four rows exist in the table (unfiltered)");
}
