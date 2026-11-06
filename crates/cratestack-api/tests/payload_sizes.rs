//! The payload-size gate: the bytes each representative body costs on the
//! wire, measured with the real codecs and compared with a committed
//! baseline, so a change that grows or shrinks a body is a reviewed diff
//! instead of a silent one.
//!
//! The bodies are the generated `Args` (request) and `Output` (response) of
//! `tests/fixtures/payload_sizes.cstack`: a product create and a money
//! transfer, both single procedure calls, and a 500-row list. Each is
//! encoded with `CborCodec` and `JsonCodec`, raw and gzipped (flate2's
//! default level), and the table is compared exactly with
//! `tests/payload_sizes.baseline`.
//!
//! The `compact-*` columns are the same bodies with integer field keys
//! (`docs/design/compact-wire-keys.md`): the named bytes are rewritten by
//! `cratestack_codec_cbor::cbor_to_compact` / `cratestack_codec_json::
//! json_to_compact` with the type tree of `KeyTables::from_schema` for the
//! fixture, taken at the root the op's request or response really has. Each
//! row also checks that the rewrite is reversible byte for byte. The
//! requests are the `Args` wrapper (`{"input": …}`), whose own key is
//! numbered too; the named columns count the wrapper's text key.
//!
//! On a deliberate change, re-baseline and commit the diff:
//!
//! ```text
//! CRATESTACK_UPDATE_PAYLOAD_SIZES=1 cargo test -p cratestack-api --test payload_sizes
//! ```
//!
//! The sizes hold for every client, not only Rust: the Dart client
//! (`cratestack_cbor`) and the TypeScript RPC client (`@cratestack/cbor`)
//! encode through this same Rust `CborCodec`. The TypeScript REST runtime's
//! `JSON.stringify` is the exception and is not measured here. Raw sizes are
//! fixed by the codecs; gzip sizes are fixed for a given flate2 version, so a
//! flate2 bump may need a re-baseline on its own.
//!
//! `docs/design/compact-wire-keys.md` and the docs site's "Reducing payload
//! size" guide cite this baseline.

use std::io::Write;
use std::path::Path;

use cratestack::include_server_schema;
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_cbor::compact::{cbor_from_compact, cbor_to_compact};
use cratestack_codec_json::JsonCodec;
use cratestack_codec_json::compact::{json_from_compact, json_to_compact};
use cratestack_core::compact::{Direction, KeyTables};
use cratestack_core::op_list::procedure_op_key;
use cratestack_core::{CratestackCodec, Schema};
use flate2::Compression;
use flate2::write::GzEncoder;
use serde::Serialize;

include_server_schema!(
    "tests/fixtures/payload_sizes.cstack",
    db = None,
    decimal = RustDecimal
);

mod payload_sizes_support;

const BASELINE: &str = "tests/payload_sizes.baseline";
const UPDATE_ENV: &str = "CRATESTACK_UPDATE_PAYLOAD_SIZES";

fn gzip_len(bytes: &[u8]) -> usize {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(bytes)
        .expect("writing to a Vec cannot fail");
    encoder
        .finish()
        .expect("writing to a Vec cannot fail")
        .len()
}

/// The fixture's schema, parsed the way the macro reads it.
fn schema() -> Schema {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/payload_sizes.cstack");
    let source = std::fs::read_to_string(path).expect("fixture is readable");
    cratestack_parser::parse_schema(&source).expect("fixture parses")
}

/// The compact size of `bytes` (a named body of `procedure`'s `dir` side),
/// after checking the rewrite undoes itself byte for byte.
struct Compact<'a> {
    tables: &'a KeyTables,
    schema: &'a Schema,
}

impl Compact<'_> {
    fn bodies(
        &self,
        procedure: &str,
        dir: Direction,
        cbor: &[u8],
        json: &[u8],
    ) -> (Vec<u8>, Vec<u8>) {
        let procedure = self
            .schema
            .procedures
            .iter()
            .find(|p| p.name == procedure)
            .expect("fixture declares the procedure");
        let op = procedure_op_key(procedure, false);
        let keys = self
            .tables
            .ops
            .iter()
            .find(|keys| keys.op == op)
            .expect("the tables hold every op");
        let node = self.tables.node(keys, dir);
        let ccbor = cbor_to_compact(self.tables, node, cbor).expect("CBOR rewrites");
        let cjson = json_to_compact(self.tables, node, json).expect("JSON rewrites");
        assert_eq!(
            cbor_from_compact(self.tables, node, &ccbor).expect("CBOR restores"),
            cbor
        );
        assert_eq!(
            json_from_compact(self.tables, node, &cjson).expect("JSON restores"),
            json
        );
        (ccbor, cjson)
    }
}

/// One table row: a body's size under each codec, raw and gzipped, named
/// and with compact keys.
fn row<T: Serialize>(
    compact: &Compact<'_>,
    label: &str,
    op: (&str, Direction),
    value: &T,
) -> String {
    let cbor = CborCodec
        .encode(value)
        .expect("generated types encode as CBOR");
    let json = JsonCodec
        .encode(value)
        .expect("generated types encode as JSON");
    let (ccbor, cjson) = compact.bodies(op.0, op.1, &cbor, &json);
    format!(
        "{label:<24} {:>8} {:>8} {:>8} {:>8} {:>13} {:>16} {:>13} {:>16}",
        cbor.len(),
        gzip_len(&cbor),
        json.len(),
        gzip_len(&json),
        ccbor.len(),
        gzip_len(&ccbor),
        cjson.len(),
        gzip_len(&cjson),
    )
}

fn table() -> String {
    let bodies = payload_sizes_support::bodies();
    let schema = schema();
    let tables = KeyTables::from_schema(&schema);
    let compact = Compact {
        tables: &tables,
        schema: &schema,
    };
    let mut lines = vec![
        "# Bytes on the wire. Generated by tests/payload_sizes.rs; do not edit by hand.".to_owned(),
        format!(
            "{:<24} {:>8} {:>8} {:>8} {:>8} {:>13} {:>16} {:>13} {:>16}",
            "body",
            "cbor",
            "cbor+gz",
            "json",
            "json+gz",
            "compact cbor",
            "compact cbor+gz",
            "compact json",
            "compact json+gz"
        ),
    ];
    use Direction::{Request, Response};
    lines.push(row(
        &compact,
        "createProduct request",
        ("createProduct", Request),
        &bodies.create_product_args,
    ));
    lines.push(row(
        &compact,
        "createProduct response",
        ("createProduct", Response),
        &bodies.product,
    ));
    lines.push(row(
        &compact,
        "transferFunds request",
        ("transferFunds", Request),
        &bodies.transfer_args,
    ));
    lines.push(row(
        &compact,
        "transferFunds response",
        ("transferFunds", Response),
        &bodies.transaction,
    ));
    lines.push(row(
        &compact,
        "listEvents request",
        ("listEvents", Request),
        &bodies.list_args,
    ));
    lines.push(row(
        &compact,
        "listEvents response",
        ("listEvents", Response),
        &bodies.events,
    ));
    lines.join("\n") + "\n"
}

#[test]
fn payload_sizes_match_the_committed_baseline() {
    let actual = table();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(BASELINE);
    if std::env::var_os(UPDATE_ENV).is_some() {
        std::fs::write(&path, &actual).expect("baseline is writable");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        actual == expected,
        "payload sizes moved.\n\n--- baseline ({BASELINE})\n{expected}\n--- now\n{actual}\n\
         If the change is deliberate, re-baseline and commit the diff:\n  \
         {UPDATE_ENV}=1 cargo test -p cratestack-api --test payload_sizes"
    );
}
