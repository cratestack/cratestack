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
