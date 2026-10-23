//! cratestack#1209 for the SQLite emitter. SQLite takes a function call in
//! `DEFAULT` only inside parentheses, so before this every function
//! default failed with `near "(": syntax error`. These tests apply the
//! emitted DDL to a real in-memory SQLite and read the defaults back the
//! way the embedded runtime decodes them.

use rusqlite::Connection;

use super::super::emit;
use super::schema;
use crate::diff::diff;

const DATASOURCE: &str = "datasource db {\n  provider = \"sqlite\"\n  \
                          url = env(\"DATABASE_URL\")\n  functions = [\"upper\"]\n}\n";

fn up(models: &str) -> String {
    let empty = schema(DATASOURCE);
    let next = schema(&format!("{DATASOURCE}\n{models}"));
    emit(&diff(&empty, &next).expect("diff should succeed")).up
}

#[test]
fn builtin_defaults_apply_and_decode_like_the_runtime() {
    let up = up(
        "model Ticket {\n  id Uuid @id @default(uuid())\n  code Cuid @default(cuid())\n  \
                 createdAt DateTime @default(now())\n  title String\n}\n",
    );
    assert!(
        !up.contains("DEFAULT now()") && !up.contains("DEFAULT uuid()"),
        "{up}"
    );

    let conn = Connection::open_in_memory().expect("sqlite");
    conn.execute_batch(&up).expect("the emitted DDL applies");
    // `cuid()` has no database default: a writer outside cratestack
    // supplies it, as the NOT NULL column requires.
    for code in ["first", "second"] {
        conn.execute("INSERT INTO tickets (code, title) VALUES (?1, 't')", [code])
            .expect("insert leaving id and created_at to their defaults");
    }
    let rows: Vec<(String, String)> = conn
        .prepare("SELECT id, created_at FROM tickets")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    for (id, created_at) in &rows {
        let id = uuid::Uuid::parse_str(id).expect("a uuid");
        assert_eq!(id.get_version_num(), 4, "{id}");
        assert_eq!(id.hyphenated().to_string(), id.to_string());
        chrono::DateTime::parse_from_rfc3339(created_at).expect("RFC 3339, as the runtime decodes");
    }
    assert_ne!(rows[0].0, rows[1].0);
}

#[test]
fn registered_functions_are_parenthesised() {
    let up = up("model Ticket {\n  id Int @id\n  code String @default(upper('x'))\n}\n");
    assert!(up.contains("DEFAULT (upper('x'))"), "{up}");
    let conn = Connection::open_in_memory().expect("sqlite");
    conn.execute_batch(&up).expect("the emitted DDL applies");
    conn.execute("INSERT INTO tickets (id) VALUES (1)", [])
        .unwrap();
    let code: String = conn
        .query_row("SELECT code FROM tickets", [], |row| row.get(0))
        .unwrap();
    assert_eq!(code, "X");
}

/// SQLite refuses an expression default on `ADD COLUMN` once the table has
/// rows; the migration says so where it happens.
#[test]
fn adding_a_column_with_an_expression_default_carries_a_note() {
    let prev = schema(&format!("{DATASOURCE}\nmodel T {{\n  id Int @id\n}}\n"));
    let next = schema(&format!(
        "{DATASOURCE}\nmodel T {{\n  id Int @id\n  seenAt DateTime? @default(now())\n}}\n"
    ));
    let up = emit(&diff(&prev, &next).expect("diff")).up;
    assert!(
        up.contains("Cannot add a column with non-constant default"),
        "{up}"
    );
    Connection::open_in_memory()
        .unwrap()
        .execute_batch(&format!("CREATE TABLE ts (id BLOB PRIMARY KEY);\n{up}"))
        .expect("an empty table accepts it");
}
