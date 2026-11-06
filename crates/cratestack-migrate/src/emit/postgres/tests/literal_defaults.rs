//! cratestack#1234: a literal default renders as the SQL literal it
//! stands for. A double-quoted `.cstack` string used to be written as
//! `DEFAULT "draft"`, which Postgres reads as a column reference.

use super::super::emit;
use super::schema;
use crate::diff::diff;

const DATASOURCE: &str =
    "datasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n";

#[test]
fn literals_render_as_sql_literals() {
    let empty = schema(DATASOURCE);
    let next = schema(&format!(
        "{DATASOURCE}\nenum Status {{\n  Draft\n  Live\n}}\n\nmodel Widget {{\n  id Int @id\n  \
         a String @default(\"it's \\\"x\\\"\")\n  b String @default('single')\n  \
         c Boolean @default(FALSE)\n  d Int @default(+3)\n  e Status @default(\"Live\")\n  \
         f Decimal @default(12.50)\n}}\n"
    ));
    let up = emit(&diff(&empty, &next).expect("diff")).up;
    for column in [
        r#"a TEXT NOT NULL DEFAULT 'it''s "x"'"#,
        "b TEXT NOT NULL DEFAULT 'single'",
        "c BOOLEAN NOT NULL DEFAULT false",
        "d BIGINT NOT NULL DEFAULT 3",
        "e TEXT NOT NULL DEFAULT 'Live'",
        "f NUMERIC",
    ] {
        assert!(up.contains(column), "{column}: {up}");
    }
    assert!(up.contains("DEFAULT 12.50"), "{up}");
}
