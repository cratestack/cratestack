//! cratestack#1123 (binding version 2): the `cratestackOpContracts` and
//! `cratestackClientContractSha256` constants the generated Dart package
//! carries are `cratestack_core::bound_contracts` /
//! `client_contract_digest`, the same functions the Rust macros call for
//! their own `OP_CONTRACTS` (`cratestack-pg`'s `op_contract_bound_tables`
//! pins those). A REST key of a procedure carries `/$procs/`, and `$` is an
//! interpolation in a Dart string, so it must come out escaped.

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};

const REST: &str = r#"
model Widget {
  id Int @id
  name String

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
}

type PingArgs {
  message String
}

procedure ping(args: PingArgs): PingArgs
  @allow(auth() != null)
"#;

fn constants(constants_dart: &str) -> (Vec<(String, String)>, String) {
    let start = constants_dart
        .find("const Map<String, String> cratestackOpContracts")
        .expect("cratestackOpContracts is emitted");
    let body = &constants_dart[start..];
    let body = &body[body.find('{').unwrap() + 1..body.find("};").unwrap()];
    let rows = body
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim().trim_end_matches(',').split("': '");
            let key = parts.next()?.strip_prefix('\'')?;
            let hex = parts.next()?.strip_suffix('\'')?;
            Some((key.replace("\\$", "$"), hex.to_owned()))
        })
        .collect();
    let marker = "const String cratestackClientContractSha256 = '";
    let at = constants_dart.find(marker).expect("whole-contract digest") + marker.len();
    (rows, constants_dart[at..at + 64].to_owned())
}

#[test]
fn the_package_constants_are_the_core_ones_for_rest_and_rpc() {
    for source in [REST.to_owned(), format!("transport rpc\n{REST}")] {
        let schema = cratestack_parser::parse_schema(&source).expect("parses");
        for preset in [DartPreset::Default, DartPreset::Riverpod] {
            let package = generate_package(
                &schema,
                &DartGeneratorConfig {
                    library_name: "client".to_owned(),
                    preset,
                    ..Default::default()
                },
            )
            .expect("renders");
            let file = package
                .files
                .iter()
                .find(|file| file.file_name.ends_with("constants.dart"))
                .expect("constants.dart");
            let (rows, whole) = constants(&file.contents);
            let expected: Vec<(String, String)> = cratestack_core::bound_contracts(&schema)
                .into_iter()
                .map(|(key, digest)| (key, cratestack_core::digest_hex(&digest)))
                .collect();
            assert_eq!(rows, expected, "{preset:?}");
            assert_eq!(
                whole,
                cratestack_core::digest_hex(&cratestack_core::client_contract_digest(&schema))
            );
            assert!(
                !file.contents.contains("'POST /$procs"),
                "an unescaped `$` would interpolate in Dart"
            );
        }
    }
}

/// The cross-language vector (`cratestack-cose/tests/vectors/contract.json`,
/// written and checked by `cratestack-parser`'s `contract_vectors`): the
/// constants the generated package carries are the vector's digests.
#[test]
fn the_package_constants_are_the_cross_language_vectors() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cratestack-cose/tests/vectors/contract.json"
    );
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("vectors")).unwrap();
    for case in doc["cases"].as_array().unwrap() {
        let schema =
            cratestack_parser::parse_schema(case["schema"].as_str().unwrap()).expect("parses");
        let package = generate_package(
            &schema,
            &DartGeneratorConfig {
                library_name: "client".to_owned(),
                ..Default::default()
            },
        )
        .expect("renders");
        let file = package
            .files
            .iter()
            .find(|file| file.file_name.ends_with("constants.dart"))
            .expect("constants.dart");
        let (rows, whole) = constants(&file.contents);
        let expected: Vec<(String, String)> = case["bound"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row[0].as_str().unwrap().to_owned(),
                    row[1].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(rows, expected, "{}", case["name"]);
        assert_eq!(
            whole,
            case["client_contract"].as_str().unwrap(),
            "{}",
            case["name"]
        );
    }
}
