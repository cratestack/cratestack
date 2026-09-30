//! `cratestack-cose/tests/vectors/contract.json` (cratestack#1123): a fixture
//! schema, the canonical op-contract JSON of every op, each op's digest and
//! the client contract digest, so a non-Rust implementation (the TypeScript
//! and Dart generators today, a sealer tomorrow) can check its own
//! derivation against this one. The TypeScript and Dart generators' tests
//! read the same file.
//!
//! Regenerate with `CRATESTACK_CONTRACT_WRITE_VECTORS=1 cargo test -p
//! cratestack-parser --test contract_vectors`; any diff in the file is a
//! change of derivation, which moves `OP_CONTRACT_DOMAIN`.

use std::path::PathBuf;

use cratestack_core::{
    Schema, bound_contracts, client_contract_digest, digest_hex, op_contract_json, op_keys,
};
use serde_json::{Value, json};

const MODELS: &str = r#"
enum Kind {
  Small
  Large
}

model Widget {
  id Int @id
  name String @length(min: 1, max: 40)
  note String?
  tier Int @default(1)
  kind Kind
  secret String @server_only

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@index([name])
}

type PingArgs {
  message String
}

type PingReply {
  echo String
}

procedure ping(args: PingArgs): PingReply
  @allow(auth() != null)
"#;

fn path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cratestack-cose/tests/vectors/contract.json")
}

fn case(name: &str, source: &str) -> Value {
    let schema: Schema = cratestack_parser::parse_schema(source).expect("fixture parses");
    let ops: Vec<Value> = op_keys(&schema)
        .into_iter()
        .map(|key| {
            let canonical = op_contract_json(&schema, &key).expect("listed op");
            let digest = cratestack_core::op_contract_digest(&schema, &key).expect("listed op");
            json!({ "key": key, "canonical": canonical, "digest": digest_hex(&digest) })
        })
        .collect();
    let bound: Vec<Value> = bound_contracts(&schema)
        .into_iter()
        .map(|(key, digest)| json!([key, digest_hex(&digest)]))
        .collect();
    json!({
        "name": name,
        "schema": source,
        "ops": ops,
        "bound": bound,
        "client_contract": digest_hex(&client_contract_digest(&schema)),
    })
}

fn document() -> Value {
    json!({
        "description": "Op contract digests (cratestack#1123, binding version 2). \
            `canonical` is the exact byte string an op digest is taken over: \
            SHA-256(op_contract_domain || canonical). `client_contract` is \
            SHA-256(client_contract_domain || JSON([[key, digest hex] sorted by key])). \
            `bound` is what a client stamps: each op's digest plus, for `transport rpc`, \
            a `batch` row holding `client_contract`.",
        "domain_note": "Both domain tags end with one NUL byte, written `\\0` here.",
        "op_contract_domain": "cratestack/op-contract/v1\\0",
        "client_contract_domain": "cratestack/client-contract/v1\\0",
        "cases": [
            case("rest", MODELS),
            case("rpc", &format!("transport rpc\n{MODELS}")),
        ],
    })
}

#[test]
fn the_checked_in_vectors_are_what_this_build_derives() {
    let derived = format!("{}\n", serde_json::to_string_pretty(&document()).unwrap());
    if std::env::var("CRATESTACK_CONTRACT_WRITE_VECTORS").as_deref() == Ok("1") {
        std::fs::write(path(), &derived).expect("write vectors");
    }
    let checked_in = std::fs::read_to_string(path()).expect("contract.json exists");
    assert_eq!(
        checked_in, derived,
        "contract.json is stale: a derivation change needs a new domain tag, then \
         CRATESTACK_CONTRACT_WRITE_VECTORS=1"
    );
}

#[test]
fn the_vectors_cover_a_server_only_field_and_the_dropped_attributes() {
    let doc = document();
    let create = doc["cases"][1]["ops"]
        .as_array()
        .unwrap()
        .iter()
        .find(|op| op["key"] == "model.Widget.create")
        .unwrap();
    let canonical = create["canonical"].as_str().unwrap();
    assert!(!canonical.contains("secret"), "server-only is on no wire");
    assert!(!canonical.contains("@@allow") && !canonical.contains("@length"));
    assert!(canonical.contains("@default(1)") && canonical.contains("Small"));
}

/// The derivation the file documents, recomputed from the file alone with no
/// cratestack code: what a non-Rust implementation would do.
#[test]
fn the_digests_follow_from_the_canonical_strings_alone() {
    use sha2::{Digest, Sha256};
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(path()).unwrap()).unwrap();
    let domain = |field: &str| doc[field].as_str().unwrap().replace("\\0", "\0");
    let hex = |bytes: &[u8]| -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() };
    for case in doc["cases"].as_array().unwrap() {
        let mut table: Vec<[String; 2]> = Vec::new();
        for op in case["ops"].as_array().unwrap() {
            let mut hasher = Sha256::new();
            hasher.update(domain("op_contract_domain"));
            hasher.update(op["canonical"].as_str().unwrap());
            let digest = hex(&hasher.finalize());
            assert_eq!(digest, op["digest"].as_str().unwrap(), "{}", op["key"]);
            table.push([op["key"].as_str().unwrap().to_owned(), digest]);
        }
        table.sort();
        let mut hasher = Sha256::new();
        hasher.update(domain("client_contract_domain"));
        hasher.update(serde_json::to_vec(&table).unwrap());
        assert_eq!(
            hex(&hasher.finalize()),
            case["client_contract"].as_str().unwrap()
        );
    }
}
