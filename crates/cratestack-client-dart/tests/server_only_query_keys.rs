//! The generated `FindMany` surface never offers a `@server_only` field as
//! a filter or sort key. The server refuses one exactly as an undeclared
//! name (`queryable_model_fields` in `cratestack-macros`), so a
//! `<Model>Where` field or `<Model>SortField` variant naming it could only
//! ever fail. Its public twin stays, so an empty class or enum cannot pass.

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};

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

/// The body of `class <name> ` (or `enum <name> `), wherever the preset
/// put it.
fn declaration(sources: &[String], keyword: &str, name: &str) -> String {
    let head = format!("{keyword} {name} ");
    let source = sources
        .iter()
        .find(|source| source.contains(&head))
        .unwrap_or_else(|| panic!("no `{head}` in any generated file"));
    let start = source.find(&head).expect("found above");
    let end = source[start..]
        .find("\n}")
        .map_or(source.len(), |offset| start + offset);
    source[start..end].to_owned()
}

#[test]
fn find_many_where_and_sort_field_never_name_a_server_only_field() {
    let schema = cratestack_parser::parse_schema(SCHEMA).expect("schema should parse");
    for preset in [DartPreset::Default, DartPreset::Riverpod] {
        let config = DartGeneratorConfig {
            preset,
            ..DartGeneratorConfig::default()
        };
        let package = generate_package(&schema, &config).expect("package should generate");
        let sources: Vec<String> = package.files.into_iter().map(|f| f.contents).collect();

        let where_class = declaration(&sources, "class", "AccountWhere");
        let sort_enum = declaration(&sources, "enum", "AccountSortField");
        for twin in ["name", "nickname"] {
            assert!(
                where_class.contains(&format!("'{twin}'")),
                "{preset:?}: AccountWhere must keep `{twin}`: {where_class}"
            );
            assert!(
                sort_enum.contains(&format!("{twin}('{twin}')")),
                "{preset:?}: AccountSortField must keep `{twin}`: {sort_enum}"
            );
        }
        for server_only in ["secret", "recovery"] {
            assert!(
                !where_class.contains(server_only),
                "{preset:?}: AccountWhere names @server_only `{server_only}`: {where_class}"
            );
            assert!(
                !sort_enum.contains(server_only),
                "{preset:?}: AccountSortField names @server_only `{server_only}`: {sort_enum}"
            );
        }
    }
}
