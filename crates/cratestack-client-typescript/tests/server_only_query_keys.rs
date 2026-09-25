//! The generated `FindMany` surface never offers a `@server_only` field as
//! a filter or sort key. The server refuses one exactly as an undeclared
//! name (`queryable_model_fields` in `cratestack-macros`), so a
//! `<Model>Where` field or `<Model>SortField` member naming it could only
//! ever fail. The TypeScript generator already left such fields out
//! (`scalar_model_fields` in `src/types.rs`); this pins it. The public
//! twins stay, so an empty interface or union cannot pass.

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

const SCHEMA: &str = r#"
model Account {
  id Int @id
  name String
  nickname String?
  secret String @server_only
  recovery String? @server_only
}

procedure findAccounts(query: FindMany<Account>): Account[]
"#;

#[test]
fn find_many_where_and_sort_field_never_name_a_server_only_field() {
    let schema = cratestack_parser::parse_schema(SCHEMA).expect("schema should parse");
    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("package should generate");
    let sources: Vec<(String, String)> = package
        .files
        .into_iter()
        .map(|file| (file.file_name, file.contents))
        .collect();

    let declares = |name: &str| sources.iter().any(|(_, source)| source.contains(name));
    assert!(declares("AccountWhere"), "no AccountWhere generated");
    assert!(
        declares("AccountSortField"),
        "no AccountSortField generated"
    );
    assert!(declares("nickname"), "the public twin `nickname` must stay");
    for (file, source) in &sources {
        for server_only in ["secret", "recovery"] {
            assert!(
                !source.contains(server_only),
                "{file} names @server_only `{server_only}`:\n{source}"
            );
        }
    }
}
