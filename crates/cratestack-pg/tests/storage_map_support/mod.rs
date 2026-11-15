//! Shared by the `storage_map*.rs` binaries (cratestack#1375): the tables, made
//! by hand under the mapped names the fixture declares.
#![allow(dead_code)]

use cratestack::sqlx::{PgPool, query};

pub async fn reset_schema(pool: &PgPool) {
    for sql in [
        "DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, map_deposit, map_bank",
        "CREATE TABLE map_bank (
            bank_id BIGINT PRIMARY KEY,
            bank_code TEXT NOT NULL UNIQUE,
            display_name TEXT NOT NULL,
            ssn_enc TEXT NOT NULL,
            tax_ref TEXT NOT NULL,
            region TEXT NOT NULL,
            rev BIGINT NOT NULL DEFAULT 0
        )",
        "CREATE TABLE map_deposit (
            id BIGINT PRIMARY KEY,
            bank_ref BIGINT NOT NULL REFERENCES map_bank (bank_id),
            memo_text TEXT NOT NULL,
            amount_minor BIGINT NOT NULL,
            UNIQUE (bank_ref, memo_text)
        )",
    ] {
        query(sql).execute(pool).await.expect(sql);
    }
}

/// Banks 1..=3 (`hidden` is 3, which the read policy refuses), two deposits on bank 1.
pub async fn seed(pool: &PgPool) {
    for sql in [
        "INSERT INTO map_bank VALUES (1, 'AAA', 'Alpha', 'ssn-1', 'tax-1', 'eu', 0)",
        "INSERT INTO map_bank VALUES (2, 'BBB', 'Beta', 'ssn-2', 'tax-2', 'us', 0)",
        "INSERT INTO map_bank VALUES (3, 'CCC', 'hidden', 'ssn-3', 'tax-3', 'eu', 0)",
        "INSERT INTO map_deposit VALUES (10, 1, 'rent', 500)",
        "INSERT INTO map_deposit VALUES (11, 1, 'salary', 900)",
    ] {
        query(sql).execute(pool).await.expect(sql);
    }
}
