//! `migrate baseline` over a database with legacy names, then a changed
//! `@@map` / `@map` (cratestack#1375): the snapshot the baseline writes must
//! make the change a refusal, not a drop and a create.

use std::fs;

use tempfile::TempDir;

use super::{block_on, connect, exec, isolated_test_db, migration_dirs, write_schema};
use crate::cli_types::{BaselineBackendArg, MigrateBackendArg};
use crate::migrate::{handle_baseline, handle_diff};

fn schema(table: &str, column: &str, extra: &str) -> String {
    format!(
        "datasource db {{\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}}\n\n\
         model Bank {{\n  id Int @id\n  ssn String @map(\"{column}\")\n  @@map(\"{table}\")\n  {extra}\n}}\n"
    )
}

fn diff(dir: &TempDir, source: &str, out: &std::path::Path) -> anyhow::Result<()> {
    let path = write_schema(dir, source);
    handle_diff(
        path,
        out.to_owned(),
        MigrateBackendArg::Postgres,
        "next".to_owned(),
        false,
    )
}

#[test]
fn a_storage_name_change_after_baseline_is_refused_and_a_rename_is_allowed() {
    let Some(url) = isolated_test_db("storage_map") else {
        return;
    };
    let dir = TempDir::new().expect("tempdir");
    let out = dir.path().join("migrations");
    block_on(async {
        let pool = connect(&url).await;
        exec(
            &pool,
            "CREATE TABLE legacy_banks (id BIGINT NOT NULL PRIMARY KEY, ssn_enc TEXT NOT NULL);",
        )
        .await;
    });
    let adopted = schema("legacy_banks", "ssn_enc", "");
    handle_baseline(
        write_schema(&dir, &adopted),
        url,
        out.clone(),
        BaselineBackendArg::Postgres,
        false,
    )
    .expect("baseline");
    let backend_dir = out.join("postgres");
    let snapshot = fs::read_to_string(backend_dir.join("schema.snapshot.json")).unwrap();
    assert!(snapshot.contains("\"model\": \"Bank\"") || snapshot.contains("\"model\":\"Bank\""));

    diff(&dir, &adopted, &out).expect("unchanged schema: no changes");
    assert!(migration_dirs(&backend_dir).is_empty());

    let table = diff(&dir, &schema("bank_v2", "ssn_enc", ""), &out)
        .expect_err("a changed @@map must be refused");
    let table = format!("{table:#}");
    assert!(
        table.contains("@@rename(from = \"legacy_banks\")"),
        "{table:#}"
    );
    let column = diff(&dir, &schema("legacy_banks", "ssn_cipher", ""), &out)
        .expect_err("a changed @map must be refused");
    assert!(
        format!("{column:#}").contains("@rename(from = \"ssn_enc\")"),
        "{column:#}"
    );
    assert!(
        migration_dirs(&backend_dir).is_empty(),
        "a refusal writes nothing"
    );

    diff(
        &dir,
        &schema("bank_v2", "ssn_enc", "@@rename(from = \"legacy_banks\")"),
        &out,
    )
    .expect("a declared rename");
    assert_eq!(migration_dirs(&backend_dir).len(), 1);
}
