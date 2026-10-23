//! cratestack#1209: function-shaped defaults render as Postgres can run
//! them. cratestack's own `now()` and `uuid()` become `now()` and
//! `gen_random_uuid()`; `cuid()` has no database equivalent and gets no
//! `DEFAULT`; a registered function is written verbatim; `auth().field`
//! never reaches DDL.

use super::super::emit;
use super::schema;
use crate::diff::diff;

const DATASOURCE: &str = "datasource db {\n  provider = \"postgresql\"\n  \
                          url = env(\"DATABASE_URL\")\n  functions = [\"next_ticket_ref\"]\n}\n";

fn up(models: &str) -> String {
    let empty = schema(DATASOURCE);
    let next = schema(&format!("{DATASOURCE}\n{models}"));
    emit(&diff(&empty, &next).expect("diff should succeed")).up
}

#[test]
fn builtins_render_as_their_postgres_equivalents() {
    let up = up(
        "model Ticket {\n  id Uuid @id @default(uuid())\n  code Cuid @default(cuid())\n  \
                 reference String @default(uuid())\n  createdAt DateTime @default(now())\n}\n",
    );
    assert!(
        up.contains("id UUID NOT NULL DEFAULT gen_random_uuid()"),
        "{up}"
    );
    assert!(
        up.contains("reference TEXT NOT NULL DEFAULT gen_random_uuid()"),
        "{up}"
    );
    assert!(
        up.contains("created_at TIMESTAMPTZ NOT NULL DEFAULT now()"),
        "{up}"
    );
    assert!(
        up.contains("code TEXT NOT NULL,"),
        "cuid() gets no DEFAULT: {up}"
    );
    for absent in ["uuid()\n", "DEFAULT uuid()", "cuid()"] {
        assert!(!up.contains(absent), "{absent}: {up}");
    }
}

#[test]
fn registered_functions_are_written_verbatim() {
    let up =
        up("model Ticket {\n  id Int @id\n  reference String @default(next_ticket_ref())\n}\n");
    assert!(
        up.contains("reference TEXT NOT NULL DEFAULT next_ticket_ref()"),
        "{up}"
    );
}

/// Before cratestack#1209 this emitted `DEFAULT auth().id`.
#[test]
fn auth_defaults_never_reach_ddl() {
    let up = up(
        "auth Principal {\n  id String\n}\n\nmodel Doc {\n  id Int @id\n  \
                 owner String @default(auth().id)\n}\n",
    );
    assert!(up.contains("owner TEXT NOT NULL,"), "{up}");
    assert!(!up.contains("auth()"), "{up}");
}

#[test]
fn switching_to_cuid_drops_the_old_database_default() {
    let prev = schema(&format!(
        "{DATASOURCE}\nmodel T {{\n  id Int @id\n  ref String @default(uuid())\n}}\n"
    ));
    let next = schema(&format!(
        "{DATASOURCE}\nmodel T {{\n  id Int @id\n  ref String @default(cuid())\n}}\n"
    ));
    let migration = emit(&diff(&prev, &next).expect("diff"));
    assert!(
        migration.up.contains("ALTER COLUMN ref DROP DEFAULT;"),
        "{}",
        migration.up
    );
    assert!(
        migration
            .down
            .contains("ALTER COLUMN ref SET DEFAULT gen_random_uuid();"),
        "{}",
        migration.down
    );
}
