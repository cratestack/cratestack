//! Tests for `diff` split by topic into sibling submodules to stay
//! under the 200-LoC budget.

mod basic;
mod indexes;

use cratestack_core::Schema;
use cratestack_parser::parse_schema;

pub(super) fn schema(source: &str) -> Schema {
    parse_schema(source).expect("schema should parse")
}

const DATASOURCE: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}
"#;

pub(super) fn with_models(models: &str) -> String {
    format!("{DATASOURCE}{models}")
}
