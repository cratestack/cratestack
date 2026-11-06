//! An ordinal enum (`Pending = 1`) in the generated Dart client: constants
//! unchanged, `toWire` sends the number, `fromWire` takes the number or the
//! name (`docs/design/enum-ordinals.md` §2.2–§2.3), in every preset.
//! `just verify-dart` runs the generated code itself, from
//! `tests/fixtures/ordinal_enums.cstack`.

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

const SCHEMA: &str = r#"
enum Status {
  Pending = 1
  Approved = 2
  Refunded = 5
  @@reserved(4)
}

enum Role {
  admin
  member
}

model Payment {
  id Int @id
  status Status
  role Role
}
"#;

fn package(preset: DartPreset) -> GeneratedDartPackage {
    let schema = cratestack_parser::parse_schema(SCHEMA).expect("parses");
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "ordinal_client".to_owned(),
            base_path: "/api".to_owned(),
            preset,
            ..DartGeneratorConfig::default()
        },
    )
    .expect("renders")
}

fn all_dart(package: &GeneratedDartPackage) -> String {
    package
        .files
        .iter()
        .filter(|file| file.file_name.ends_with(".dart"))
        .map(|file| file.contents.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn an_ordinal_enum_sends_its_number_and_reads_either_form() {
    for preset in [DartPreset::Default, DartPreset::Riverpod] {
        let dart = all_dart(&package(preset));
        for needle in [
            "pending(1, 'Pending'),",
            "refunded(5, 'Refunded');",
            "const Status(this.wireValue, this.wireName);",
            "final int wireValue;",
            "case 1:\n      case 'Pending':\n        return Status.pending;",
            "Object toWire() => wireValue;",
        ] {
            assert!(dart.contains(needle), "{preset:?}: missing {needle:?}");
        }
        // A string enum in the same schema is generated as before.
        assert!(dart.contains("admin('admin'),"), "{preset:?}");
        assert!(dart.contains("const Role(this.wireName);"), "{preset:?}");
    }
}
