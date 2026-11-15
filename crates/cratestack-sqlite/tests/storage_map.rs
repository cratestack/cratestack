//! `@@map` / `@map` through the embedded backend (cratestack#1375): the DDL the
//! runtime derives, and every operation, use the mapped table and columns while
//! the Rust API keeps the field names. Real SQLite in memory, no skip path.

use cratestack::RusqliteRuntime;
use cratestack::include_embedded_schema;
use cratestack::rusqlite;
use cratestack_rusqlite::{ModelDelegate, ddl::create_table_sql};

include_embedded_schema!("tests/fixtures/storage_map.cstack");

use cratestack_schema::models::{LiteBank, LiteDeposit};
use cratestack_schema::{
    CreateLiteBankInput, CreateLiteDepositInput, LITE_BANK_MODEL, LITE_DEPOSIT_MODEL,
    UpdateLiteBankInput, lite_bank, lite_deposit,
};

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    let ddl = [
        create_table_sql(&LITE_BANK_MODEL),
        create_table_sql(&LITE_DEPOSIT_MODEL),
    ]
    .join(";\n");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&format!("{ddl};")).expect("apply DDL");
            Ok(())
        })
        .expect("connection");
    runtime
}

fn banks(runtime: &RusqliteRuntime) -> ModelDelegate<'_, LiteBank, i64> {
    ModelDelegate::<LiteBank, i64>::new(runtime, &LITE_BANK_MODEL)
}

fn bank(id: i64, code: &str, name: &str, region: &str) -> CreateLiteBankInput {
    CreateLiteBankInput {
        id,
        code: code.to_owned(),
        displayName: name.to_owned(),
        region: region.to_owned(),
    }
}

fn columns_of(runtime: &RusqliteRuntime, table: &str) -> Vec<String> {
    runtime
        .with_connection(|conn| {
            let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
            let names = statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(names)
        })
        .expect("table_info")
}

#[test]
fn the_ddl_creates_the_mapped_tables_and_columns() {
    let runtime = setup();
    assert_eq!(
        columns_of(&runtime, "lite_bank"),
        ["bank_id", "bank_code", "display_name", "region"]
    );
    assert_eq!(
        columns_of(&runtime, "lite_deposit"),
        ["id", "bank_ref", "amount_minor"]
    );
    assert!(
        columns_of(&runtime, "lite_banks").is_empty(),
        "no derived table"
    );
}

#[test]
fn create_find_filter_order_update_and_delete_use_the_mapped_names() {
    let runtime = setup();
    let delegate = banks(&runtime);
    for (id, code, name, region) in [
        (1, "AAA", "Alpha", "eu"),
        (2, "BBB", "Beta", "us"),
        (3, "CCC", "Gamma", "eu"),
    ] {
        delegate
            .create(bank(id, code, name, region))
            .run()
            .expect("create");
    }

    let stored: String = runtime
        .with_connection(|conn| {
            Ok(conn.query_row(
                "SELECT bank_code || '/' || display_name FROM lite_bank WHERE bank_id = ?1",
                rusqlite::params![2],
                |row| row.get(0),
            )?)
        })
        .expect("raw read under the mapped names");
    assert_eq!(stored, "BBB/Beta");

    let found = delegate.find_unique(1).run().expect("find").expect("row");
    assert_eq!(
        (found.code.as_str(), found.displayName.as_str()),
        ("AAA", "Alpha")
    );

    let eu = delegate
        .find_many()
        .where_(lite_bank::region().eq("eu"))
        .order_by(lite_bank::displayName().desc())
        .run()
        .expect("filtered, ordered find_many");
    assert_eq!(eu.iter().map(|b| b.id).collect::<Vec<_>>(), [3, 1]);
    let by_code = delegate
        .find_many()
        .where_(lite_bank::code().eq("BBB"))
        .run()
        .expect("filter on a mapped column");
    assert_eq!(by_code.len(), 1);

    let updated = delegate
        .update(2)
        .set(UpdateLiteBankInput {
            displayName: Some("Beta 2".to_owned()),
            ..Default::default()
        })
        .run()
        .expect("update");
    assert_eq!(updated.displayName, "Beta 2");
    delegate.delete(3).run().expect("delete");
    assert!(delegate.find_unique(3).run().expect("find").is_none());
}

#[test]
fn a_relation_between_mapped_models_includes_and_orders() {
    let runtime = setup();
    banks(&runtime)
        .create(bank(1, "AAA", "Alpha", "eu"))
        .run()
        .expect("bank");
    let deposits = ModelDelegate::<LiteDeposit, i64>::new(&runtime, &LITE_DEPOSIT_MODEL);
    for (id, amount) in [(10, 500), (11, 900)] {
        deposits
            .create(CreateLiteDepositInput {
                id,
                bankId: 1,
                amount,
            })
            .run()
            .expect("deposit");
    }
    let pairs = deposits
        .find_many()
        .include(lite_deposit::bank().as_include())
        .order_by(lite_deposit::amount().desc())
        .run()
        .expect("include through the mapped foreign key");
    let seen: Vec<_> = pairs
        .iter()
        .map(|(d, b)| (d.amount, b.as_ref().map(|b| b.code.clone())))
        .collect();
    assert_eq!(
        seen,
        [(900, Some("AAA".to_owned())), (500, Some("AAA".to_owned()))]
    );
}
