//! `@@map` / `@map` never reach a generated WireMock stub (cratestack#1375): the
//! stubs mock the REST routes and RPC op ids, so the same schema with and
//! without the storage markers generates byte-identical mappings.

use cratestack_mock_wiremock::{WireMockGeneratorConfig, generate_package};

fn schema(mapped: bool, rpc: bool) -> String {
    let (table, id, code) = if mapped {
        (
            "@@map(\"map_bank\")",
            "@map(\"bank_id\")",
            "@map(\"bank_code\")",
        )
    } else {
        ("", "", "")
    };
    format!(
        r#"{transport}
model Bank {{
  id Int @id {id}
  code String @unique {code}
  displayName String

  {table}
  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@allow("update", auth() != null)
  @@allow("delete", auth() != null)
}}
"#,
        transport = if rpc { "transport rpc\n" } else { "" },
    )
}

fn files(source: &str) -> Vec<(String, String)> {
    let schema = cratestack_parser::parse_schema(source).expect("schema should parse");
    generate_package(&schema, &WireMockGeneratorConfig::default())
        .expect("generation succeeds")
        .files
        .into_iter()
        .map(|file| (file.file_name, file.contents))
        .collect()
}

#[test]
fn storage_markers_do_not_change_a_generated_wiremock_package() {
    for rpc in [false, true] {
        let mapped = schema(true, rpc);
        assert!(mapped.contains("@@map(") && mapped.contains("@map("));
        let with = files(&mapped);
        let without = files(&schema(false, rpc));
        assert!(
            with.iter()
                .any(|(name, _)| name.starts_with("mappings/model.Bank."))
        );
        assert_eq!(with, without, "rpc={rpc}");
        let text: String = with.iter().map(|(_, contents)| contents.as_str()).collect();
        for storage in ["map_bank", "bank_id", "bank_code"] {
            assert!(!text.contains(storage), "`{storage}` leaked (rpc={rpc})");
        }
    }
}
