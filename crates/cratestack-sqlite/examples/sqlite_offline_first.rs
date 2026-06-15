//! Offline-first multi-model SQLite demo, shaped like a banking app.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example sqlite_offline_first -p cratestack
//! ```
//!
//! Covers what a real app needs day-to-day:
//!
//! - persistent SQLite file on disk (the runtime opens and reopens it)
//! - two related models (`Account` and `Transfer`)
//! - `Decimal` round-trip for money — precision-preserving via TEXT storage
//! - filtering, ordering, paging
//! - partial updates via the patch-style update input
//!
//! Relations are app-level here (the on-device renderer doesn't yet
//! support relation-quantifier policy clauses — there are no policies on
//! device by design), so we join with regular filter expressions.

use std::path::PathBuf;

use cratestack::include_embedded_schema;
use cratestack::{Decimal, RusqliteRuntime, rusqlite_backend::ddl::create_table_sql};
use cratestack_rusqlite::ModelDelegate;

include_embedded_schema!("examples/sqlite_offline_first.cstack");

use cratestack_schema::models::{Account, Transfer};
use cratestack_schema::{
    ACCOUNT_MODEL, CreateAccountInput, CreateTransferInput, TRANSFER_MODEL, UpdateAccountInput,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open a file-backed database in the system temp dir so reruns
    // illustrate the "data survives app restart" property. On a real
    // device you'd resolve a path the platform considers persistent
    // (`Application.documentsDirectory` on iOS, `Context.getFilesDir()`
    // on Android).
    let db_path: PathBuf = std::env::temp_dir().join("cratestack_offline_first.sqlite");
    // For the example, start fresh each run so output stays predictable.
    let _ = std::fs::remove_file(&db_path);
    let runtime = RusqliteRuntime::open(&db_path)?;
    println!("opened {}", db_path.display());

    // Bootstrap both tables. Apps run this once per startup; idempotent
    // via `IF NOT EXISTS`.
    runtime.with_connection(|conn| {
        conn.execute_batch(&format!(
            "{};\n{};",
            create_table_sql(&ACCOUNT_MODEL),
            create_table_sql(&TRANSFER_MODEL),
        ))
        .expect("create tables");
        Ok(())
    })?;

    let accounts = ModelDelegate::<Account, uuid::Uuid>::new(&runtime, &ACCOUNT_MODEL);
    let transfers = ModelDelegate::<Transfer, uuid::Uuid>::new(&runtime, &TRANSFER_MODEL);

    // Seed two accounts. Note that `Decimal` precision is preserved
    // exactly — SQLite's BLOB column affinity keeps the TEXT-encoded
    // canonical form untouched, so `1234.5600` survives the round-trip.
    let alice_id = uuid::Uuid::new_v4();
    let bob_id = uuid::Uuid::new_v4();
    accounts
        .create(CreateAccountInput {
            id: alice_id,
            ownerName: "Alice".to_string(),
            balance: "1234.5600".parse::<Decimal>().expect("valid decimal literal"),
            active: true,
            openedAt: chrono::Utc::now(),
        })
        .run()?;
    accounts
        .create(CreateAccountInput {
            id: bob_id,
            ownerName: "Bob".to_string(),
            balance: "42.00".parse::<Decimal>().expect("valid decimal literal"),
            active: true,
            openedAt: chrono::Utc::now(),
        })
        .run()?;

    // Record a few transfers against Alice's account.
    for (amount, memo) in [
        ("-10.00", "coffee"),
        ("-150.99", "groceries"),
        ("2500.00", "salary"),
    ] {
        transfers
            .create(CreateTransferInput {
                id: uuid::Uuid::new_v4(),
                accountId: alice_id,
                amount: amount.parse::<Decimal>().expect("valid decimal literal"),
                memo: memo.to_string(),
                createdAt: chrono::Utc::now(),
            })
            .run()?;
    }

    // Read & filter: list Alice's transfers, newest first, capped at 2.
    let recent = transfers
        .find_many()
        .where_(cratestack_schema::transfer::accountId().eq(alice_id))
        .order_by(cratestack_schema::transfer::createdAt().desc())
        .limit(2)
        .run()?;
    println!("\nAlice's two most recent transfers:");
    for transfer in &recent {
        println!("  {:>10}  {}", transfer.amount, transfer.memo);
    }

    // Partial update: deactivate Bob's account without touching other fields.
    let updated = accounts
        .update(bob_id)
        .set(UpdateAccountInput {
            active: Some(false),
            ..Default::default()
        })
        .run()?;
    println!(
        "\nBob's account is now active={} (balance stayed {})",
        updated.active, updated.balance
    );

    // Cross-check: find every active account.
    let active = accounts
        .find_many()
        .where_(cratestack_schema::account::active().is_true())
        .order_by(cratestack_schema::account::ownerName().asc())
        .run()?;
    println!("\nactive accounts:");
    for account in &active {
        println!("  {:<10} {}", account.ownerName, account.balance);
    }

    // The whole script is sync — no tokio runtime spun up anywhere.
    Ok(())
}
