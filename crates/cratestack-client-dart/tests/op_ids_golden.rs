//! cratestack#1123: the op ids the generated Dart RPC clients print, pinned
//! as literals for a schema with `@@internal("create")`, so moving them out
//! of the templates cannot change one.

use std::collections::BTreeSet;

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};

const SCHEMA: &str = r#"
transport rpc

model Widget {
  id Int @id
  name String

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@allow("update", auth() != null)
  @@allow("delete", auth() != null)
  @@internal("create")
}

model Note {
  id Int @id
  body String

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@allow("update", auth() != null)
  @@allow("delete", auth() != null)
}

type PingArgs {
  message String
}

procedure ping(args: PingArgs): PingArgs
  @allow(auth() != null)

mutation procedure bump(args: PingArgs): PingArgs
  @allow(auth() != null)
"#;

/// Every `model.<Name>.<verb>` and `procedure.<name>` op id printed in a
/// quoted string anywhere in the generated files.
fn op_ids<'a>(files: impl Iterator<Item = &'a str>) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for text in files {
        for quote in ['"', '\''] {
            for piece in text.split(quote) {
                let id = piece.trim();
                let is_op = (id.starts_with("model.") && id.matches('.').count() == 2)
                    || (id.starts_with("procedure.") && id.matches('.').count() == 1);
                if is_op
                    && id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
                {
                    ids.insert(id.to_owned());
                }
            }
        }
    }
    ids
}

const EXPECTED: [&str; 10] = [
    "model.Note.create",
    "model.Note.delete",
    "model.Note.get",
    "model.Note.list",
    "model.Note.update",
    "model.Widget.delete",
    "model.Widget.get",
    "model.Widget.list",
    "model.Widget.update",
    "procedure.bump",
];

fn config(preset: DartPreset) -> DartGeneratorConfig {
    DartGeneratorConfig {
        library_name: "client".to_owned(),
        base_path: "/api".to_owned(),
        template_dir: None,
        preset,
        schema_sha256: "deadbeef".to_owned(),
        native_cbor: false,
    }
}

#[test]
fn the_rpc_clients_print_exactly_the_op_list() {
    let schema = cratestack_parser::parse_schema(SCHEMA).expect("schema parses");
    let mut expected: BTreeSet<String> = EXPECTED.iter().map(|s| (*s).to_owned()).collect();
    expected.insert("procedure.ping".to_owned());
    assert_eq!(
        expected,
        cratestack_core::op_keys(&schema).into_iter().collect(),
        "the literal list drifted from the op list"
    );
    for preset in [DartPreset::Default, DartPreset::Riverpod] {
        let package = generate_package(&schema, &config(preset)).expect("renders");
        let printed = op_ids(package.files.iter().map(|f| f.contents.as_str()));
        assert_eq!(printed, expected, "{preset:?}");
    }
}
