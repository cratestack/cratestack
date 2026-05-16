//! Shared helpers + integration tests for the Postgres emitter, split
//! by topic into sibling submodules to stay under the 200-LoC budget.

mod checks;
mod columns;
mod create;
mod enums;
mod renames;

use cratestack_core::Schema;
use cratestack_parser::parse_schema;

pub(super) fn schema(source: &str) -> Schema {
    parse_schema(source).expect("schema should parse")
}

pub(super) fn with_models(models: &str) -> String {
    format!(
        r#"
datasource db {{
  provider = "postgresql"
  url = env("DATABASE_URL")
}}
{models}
"#
    )
}
