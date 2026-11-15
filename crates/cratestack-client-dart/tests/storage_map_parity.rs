//! `@@map` / `@map` never reach a generated Dart client (cratestack#1375): the
//! same schema with and without the storage markers generates byte-identical
//! files, under REST and RPC, in every preset. A marker that leaked into a
//! path, an op id or a field name would show up as a difference here.

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};

fn schema(mapped: bool, rpc: bool) -> String {
    let (table, deposit_table, id, code, bank_ref) = if mapped {
        (
            "@@map(\"map_bank\")",
            "@@map(\"map_deposit\")",
            "@map(\"bank_id\")",
            "@map(\"bank_code\")",
            "@map(\"bank_ref\")",
        )
    } else {
        ("", "", "", "", "")
    };
    format!(
        r#"{transport}
model Bank {{
  id Int @id {id}
  code String @unique {code}
  displayName String
  deposits Deposit[] @relation(fields:[id],references:[bankId])

  {table}
  @@paged
  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@allow("update", auth() != null)
  @@allow("delete", auth() != null)
}}

model Deposit {{
  id Int @id
  bankId Int {bank_ref}
  memo String
  bank Bank @relation(fields:[bankId],references:[id])

  {deposit_table}
  @@unique([bankId, memo])
  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
}}
"#,
        transport = if rpc { "transport rpc\n" } else { "" },
    )
}

fn files(source: &str, preset: DartPreset) -> Vec<(String, String)> {
    let schema = cratestack_parser::parse_schema(source).expect("schema should parse");
    let config = DartGeneratorConfig {
        preset,
        ..DartGeneratorConfig::default()
    };
    generate_package(&schema, &config)
        .expect("generation succeeds")
        .files
        .into_iter()
        .map(|file| (file.file_name, file.contents))
        .collect()
}

#[test]
fn storage_markers_do_not_change_a_generated_dart_client() {
    for rpc in [false, true] {
        let mapped = schema(true, rpc);
        assert!(mapped.contains("@@map(") && mapped.contains("@map("));
        assert!(!schema(false, rpc).contains("map("));
        for preset in [DartPreset::Default, DartPreset::Riverpod] {
            let with = files(&mapped, preset);
            let without = files(&schema(false, rpc), preset);
            assert!(!with.is_empty());
            assert_eq!(with, without, "rpc={rpc} preset={preset:?}");
            let text: String = with.iter().map(|(_, contents)| contents.as_str()).collect();
            for storage in [
                "map_bank",
                "map_deposit",
                "bank_id",
                "bank_code",
                "bank_ref",
            ] {
                assert!(
                    !text.contains(storage),
                    "`{storage}` leaked (rpc={rpc} preset={preset:?})"
                );
            }
        }
    }
}
