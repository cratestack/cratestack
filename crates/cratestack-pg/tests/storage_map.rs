//! `@@map` / `@map` storage names against a real Postgres (cratestack#1375):
//! every ORM operation reads and writes the mapped table and columns while the
//! Rust API keeps the field names. The tables are hand-written under the mapped
//! names, so a generated statement that used a derived name would fail. REST,
//! audit and RPC: `storage_map_rest.rs`, `storage_map_audit.rs`,
//! `storage_map_rpc.rs`.

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{ConflictTarget, CratestackContext, Value};

include_server_schema!("tests/fixtures/storage_map.cstack", db = Postgres);

mod storage_map_support;
mod support;

use storage_map_support::{reset_schema, seed};
use support::pg;

fn operator() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
}

fn input(id: i64, code: &str, name: &str) -> cratestack_schema::CreateMapBankInput {
    cratestack_schema::CreateMapBankInput {
        id,
        code: code.to_owned(),
        displayName: name.to_owned(),
        ssn: "ssn-new".to_owned(),
        taxRef: "tax-new".to_owned(),
        region: "eu".to_owned(),
    }
}

#[tokio::test]
async fn reads_filter_sort_project_and_include_through_the_mapped_names() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;
    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    use cratestack_schema::{map_bank, map_deposit};

    let one = cool.map_bank().bind(operator()).find_unique(1).run().await;
    let one = one.expect("find_unique").expect("bank 1");
    assert_eq!(
        (one.code.as_str(), one.displayName.as_str()),
        ("AAA", "Alpha")
    );
    let denied = cool.map_bank().bind(operator()).find_unique(3).run().await;
    assert!(denied.expect("query").is_none(), "policy on a mapped field");

    let banks = cool
        .map_bank()
        .bind(operator())
        .find_many()
        .where_(map_bank::region().eq("eu"))
        .order_by(map_bank::displayName().desc())
        .limit(5)
        .run()
        .await
        .expect("find_many");
    assert_eq!(banks.iter().map(|b| b.id).collect::<Vec<_>>(), [1]);

    let projected = cool
        .map_bank()
        .bind(operator())
        .find_many()
        .order_by(map_bank::id().asc())
        .select([map_bank::displayName().column_name()])
        .run()
        .await
        .expect("select");
    assert_eq!(projected[0].value.displayName, "Alpha");
    assert!(projected[0].is_selected("display_name"));

    let pairs = cool
        .map_deposit()
        .bind(operator())
        .find_many()
        .include(map_deposit::bank().as_include())
        .order_by(map_deposit::id().asc())
        .run()
        .await
        .expect("include");
    let summary: Vec<_> = pairs
        .iter()
        .map(|(d, b)| (d.amount, b.as_ref().map(|b| b.code.clone())))
        .collect();
    assert_eq!(
        summary,
        [(500, Some("AAA".to_owned())), (900, Some("AAA".to_owned()))]
    );

    let n = cool.map_deposit().bind(operator()).aggregate().count();
    assert_eq!(n.run().await.expect("count"), 2);
    let sum = cool
        .map_deposit()
        .bind(operator())
        .aggregate()
        .sum(map_deposit::amount());
    let sum = sum.where_(map_deposit::bankId().eq(1)).run().await;
    assert_eq!(sum.expect("sum"), Some(cratestack::Decimal::from(1400_i64)));
}

#[tokio::test]
async fn writes_hit_the_mapped_columns_including_version_and_conflict_targets() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();
    use cratestack_schema::map_bank;

    let created = cool
        .map_bank()
        .create(input(1, "AAA", "Alpha"))
        .run(&ctx)
        .await;
    assert_eq!(created.expect("create").id, 1);
    let stored = query("SELECT bank_code, display_name, rev FROM map_bank WHERE bank_id = 1")
        .fetch_one(pool)
        .await
        .expect("row under the mapped names");
    assert_eq!(stored.get::<String, _>("bank_code"), "AAA");
    assert_eq!(stored.get::<i64, _>("rev"), 0);

    // Changing a unique column takes FOR UPDATE; @version bumps `rev`.
    let updated = cool
        .map_bank()
        .update(1)
        .set(cratestack_schema::UpdateMapBankInput {
            code: Some("ZZZ".to_owned()),
            displayName: Some("Alpha 2".to_owned()),
            ssn: None,
            taxRef: None,
            region: None,
        })
        .if_match(0)
        .run(&ctx)
        .await
        .expect("update");
    assert_eq!(updated.code, "ZZZ");
    assert_eq!(updated.version, 1);

    cool.map_bank()
        .create(input(2, "BBB", "Beta"))
        .run(&ctx)
        .await
        .expect("create 2");
    let touched = cool
        .map_bank()
        .update_many()
        .where_(map_bank::region().eq("eu"))
        .set(cratestack_schema::UpdateMapBankInput {
            code: None,
            displayName: None,
            ssn: None,
            taxRef: None,
            region: Some("us".to_owned()),
        })
        .run(&ctx)
        .await
        .expect("update_many");
    assert_eq!(touched.ok, 2);

    // Upsert on the primary key, then do_nothing on a mapped unique column.
    let again = cool
        .map_bank()
        .upsert(input(2, "BBB", "Beta 2"))
        .run(&ctx)
        .await;
    assert_eq!(again.expect("upsert").displayName, "Beta 2");
    let kept = cool
        .map_bank()
        .upsert(input(9, "BBB", "Other"))
        .on_conflict(ConflictTarget::Columns(&["bank_code"]))
        .do_nothing()
        .run(&ctx)
        .await
        .expect("do_nothing on a mapped unique");
    assert!(
        matches!(kept, cratestack::UpsertOutcome::Existing(ref b) if b.id == 2),
        "the existing row is returned untouched"
    );
    let names: Vec<String> = query("SELECT display_name FROM map_bank ORDER BY bank_id")
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(names, ["Alpha 2", "Beta 2"]);

    cool.map_bank()
        .delete(2)
        .if_match(2)
        .run(&ctx)
        .await
        .expect("delete");
    let left: i64 = query("SELECT count(*) FROM map_bank")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(left, 1);
}
