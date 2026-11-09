// Dart surface for keyset pagination (cratestack#1304): `PageInfo.nextCursor`,
// the `CursorPageInput` / `CursorPage<T>` procedure types, and the `cursor`
// list-query parameter — for the default and riverpod presets, REST and RPC.

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

const SCHEMA: &str = r#"
model Post {
  id Int @id
  title String
  @@paged
}

type Entry {
  id Int
  title String
}

procedure feed(page: CursorPageInput): CursorPage<Entry>
"#;

fn generate(preset: DartPreset, rpc: bool) -> GeneratedDartPackage {
    let source = if rpc {
        format!("transport rpc\n{SCHEMA}")
    } else {
        SCHEMA.to_owned()
    };
    let schema = cratestack_parser::parse_schema(&source).expect("schema parses");
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "cursor_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset,
            schema_sha256: "0".repeat(64),
            ..DartGeneratorConfig::default()
        },
    )
    .expect("generates")
}

fn file<'a>(package: &'a GeneratedDartPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|f| f.file_name == name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .contents
        .as_str()
}

#[test]
fn default_preset_has_cursor_types_and_decodes_the_procedure_result() {
    for rpc in [false, true] {
        let package = generate(DartPreset::Default, rpc);
        let models = file(&package, "lib/src/models.dart");
        assert!(
            models.contains("final String? nextCursor;"),
            "PageInfo.nextCursor"
        );
        assert!(models.contains("class CursorPageInput {"));
        assert!(models.contains("class CursorPage<T> {"));
        assert!(models.contains("final CursorPageInput page;"), "{models}");
        let all: String = package.files.iter().map(|f| f.contents.as_str()).collect();
        assert!(
            all.contains("CursorPage<Entry>.fromWire("),
            "procedure result decodes through CursorPage<Entry> (rpc={rpc})"
        );
    }
}

#[test]
fn rest_list_query_carries_cursor_and_keeps_offset_exclusive_by_documentation() {
    let package = generate(DartPreset::Default, false);
    let queries = file(&package, "lib/src/queries.dart");
    assert!(queries.contains("final String? cursor;"));
    assert!(queries.contains("if (cursor != null) query['cursor'] = cursor;"));
    assert!(
        queries.contains("cursor: base.cursor,"),
        "projection merge keeps cursor"
    );
    assert!(
        queries.contains("String? cursor,"),
        "selection toListQuery takes cursor"
    );
}

#[test]
fn riverpod_preset_has_cursor_types_and_list_query_cursor() {
    for rpc in [false, true] {
        let package = generate(DartPreset::Riverpod, rpc);
        let shared = file(&package, "lib/src/models/shared_types.dart");
        assert!(shared.contains("final String? nextCursor;"));
        assert!(shared.contains("class CursorPageInput {"));
        assert!(shared.contains("class CursorPage<T> {"));
        let procedures = file(&package, "lib/src/procedures.dart");
        assert!(procedures.contains("import 'models/shared_types.dart';"));
        assert!(procedures.contains("CursorPageInput"));
        if !rpc {
            let queries = file(&package, "lib/src/queries.dart");
            assert!(queries.contains("if (cursor != null) query['cursor'] = cursor;"));
        }
    }
}
