//! cratestack#1123 (binding version 2): the `OP_CONTRACTS` and
//! `CLIENT_CONTRACT_SHA256` constants the generated TypeScript runtimes carry
//! are `cratestack_core::bound_contracts` / `client_contract_digest`, the
//! same functions the Rust macros call for their own `OP_CONTRACTS`
//! (`cratestack-pg`'s `op_contract_bound_tables` pins those), so a sealer
//! reading them binds what the server accepts.

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

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

fn rpc() -> String {
    format!("transport rpc\n{REST}")
}

/// `(key, hex)` rows of `export const OP_CONTRACTS = { ... };`, and the
/// `CLIENT_CONTRACT_SHA256` value.
fn constants(runtime: &str) -> (Vec<(String, String)>, String) {
    let start = runtime
        .find("export const OP_CONTRACTS")
        .expect("OP_CONTRACTS is emitted");
    let body = &runtime[start..];
    let body = &body[body.find('{').unwrap() + 1..body.find("};").unwrap()];
    let rows = body
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim().trim_end_matches(',').split("\": \"");
            let key = parts.next()?.strip_prefix('"')?;
            let hex = parts.next()?.strip_suffix('"')?;
            Some((key.to_owned(), hex.to_owned()))
        })
        .collect();
    let marker = "export const CLIENT_CONTRACT_SHA256: string = \"";
    let at = runtime.find(marker).expect("CLIENT_CONTRACT_SHA256") + marker.len();
    (rows, runtime[at..at + 64].to_owned())
}

#[test]
fn the_runtime_constants_are_the_core_ones_for_rest_rpc_and_swr() {
    for (source, swr) in [
        (REST.to_owned(), false),
        (rpc(), false),
        (rpc(), true),
        (REST.to_owned(), true),
    ] {
        let schema = cratestack_parser::parse_schema(&source).expect("parses");
        let package = generate_package(
            &schema,
            &TypeScriptGeneratorConfig {
                swr,
                ..Default::default()
            },
        )
        .expect("renders");
        let runtimes: Vec<_> = package
            .files
            .iter()
            .filter(|file| file.contents.contains("export const OP_CONTRACTS"))
            .collect();
        assert!(!runtimes.is_empty(), "swr={swr}");
        let expected: Vec<(String, String)> = cratestack_core::bound_contracts(&schema)
            .into_iter()
            .map(|(key, digest)| (key, cratestack_core::digest_hex(&digest)))
            .collect();
        let client = cratestack_core::digest_hex(&cratestack_core::client_contract_digest(&schema));
        for file in runtimes {
            let (rows, whole) = constants(&file.contents);
            assert_eq!(rows, expected, "{} swr={swr}", file.file_name);
            assert_eq!(whole, client, "{} swr={swr}", file.file_name);
        }
    }
}

/// The cross-language vector (`cratestack-cose/tests/vectors/contract.json`,
/// written and checked by `cratestack-parser`'s `contract_vectors`): the
/// constants the generated runtime carries are the vector's digests.
#[test]
fn the_runtime_constants_are_the_cross_language_vectors() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cratestack-cose/tests/vectors/contract.json"
    );
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("vectors")).unwrap();
    for case in doc["cases"].as_array().unwrap() {
        let schema =
            cratestack_parser::parse_schema(case["schema"].as_str().unwrap()).expect("parses");
        let package =
            generate_package(&schema, &TypeScriptGeneratorConfig::default()).expect("renders");
        let runtime = package
            .files
            .iter()
            .find(|file| file.contents.contains("export const OP_CONTRACTS"))
            .expect("a runtime carries the constants");
        let (rows, whole) = constants(&runtime.contents);
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
