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
