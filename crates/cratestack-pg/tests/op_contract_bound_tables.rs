//! cratestack#1123 (binding version 2): the tables the macros emit are what
//! `cratestack_core` computes, the server accepts exactly the current digest
//! per op (one member each, the shape a compatible-contract lock later
//! fills), and a server-only edit to a model leaves its ops' digests alone
//! while a wire-shape edit moves only the ops it touches. No database.

use cratestack::include_server_schema;

fn digest_of(table: &[(&str, [u8; 32])], key: &str) -> [u8; 32] {
    table
        .iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("no row for {key}"))
        .1
}

macro_rules! table_suite {
    ($name:ident, $fixture:literal, rpc = $rpc:literal) => {
        mod $name {
            use super::*;
            include_server_schema!($fixture, db = Postgres);
            use cratestack_schema::{
                ACCEPTED_CONTRACTS, CLIENT_CONTRACT_SHA256, CLIENT_CONTRACT_SHA256_BYTES,
                OP_CONTRACTS,
            };

            #[test]
            fn the_emitted_tables_are_the_core_ones() {
                let schema = cratestack_parser::parse_schema_file($fixture).expect("parses");
                let bound = cratestack_core::bound_contracts(&schema);
                let emitted: Vec<(String, [u8; 32])> = OP_CONTRACTS
                    .iter()
                    .map(|(key, digest)| ((*key).to_owned(), *digest))
                    .collect();
                assert_eq!(emitted, bound);
                let keys: Vec<&str> = OP_CONTRACTS.iter().map(|(key, _)| *key).collect();
                let mut sorted = keys.clone();
                sorted.sort_unstable();
                assert_eq!(keys, sorted, "sorted by key");
                assert_eq!(
                    CLIENT_CONTRACT_SHA256_BYTES,
                    cratestack_core::client_contract_digest(&schema)
                );
                assert_eq!(
                    CLIENT_CONTRACT_SHA256,
                    cratestack_core::digest_hex(&CLIENT_CONTRACT_SHA256_BYTES)
                );
            }

            #[test]
            fn the_server_accepts_exactly_the_current_digest_per_op() {
                assert_eq!(ACCEPTED_CONTRACTS.len(), OP_CONTRACTS.len());
                for ((key, digest), (accepted_key, accepted)) in
                    OP_CONTRACTS.iter().zip(ACCEPTED_CONTRACTS)
                {
                    assert_eq!(key, accepted_key);
                    assert_eq!(accepted, &[*digest], "{key}");
                }
            }

            #[test]
            fn batch_is_a_row_under_rpc_only() {
                let batch = OP_CONTRACTS.iter().find(|(key, _)| *key == "batch");
                if $rpc {
                    assert_eq!(
                        batch.expect("rpc has batch").1,
                        CLIENT_CONTRACT_SHA256_BYTES
                    );
                } else {
                    assert!(batch.is_none());
                }
            }
        }
    };
}

table_suite!(rpc, "tests/fixtures/transport_rpc.cstack", rpc = true);
table_suite!(rest, "tests/fixtures/op_contract_rest.cstack", rpc = false);

mod old {
    cratestack::include_client_schema!("tests/fixtures/op_contract_old.cstack");
}

mod new {
    cratestack::include_server_schema!("tests/fixtures/op_contract_new.cstack", db = Postgres);
}

#[test]
fn a_server_only_model_edit_leaves_its_ops_alone_and_a_shape_edit_moves_its_own() {
    let shipped = old::cratestack_schema::OP_CONTRACTS;
    let current = new::cratestack_schema::OP_CONTRACTS;
    for verb in ["list", "get", "create", "update", "delete"] {
        let note = format!("model.Note.{verb}");
        if shipped.iter().any(|(key, _)| *key == note) {
            assert_eq!(
                digest_of(shipped, &note),
                digest_of(current, &note),
                "{note}"
            );
        }
    }
    let tag = "model.Tag.list";
    assert_ne!(digest_of(shipped, tag), digest_of(current, tag));
    // The whole-IR identity binding version 1 bound moved on all of it.
    assert_ne!(
        old::cratestack_schema::SCHEMA_SHA256_BYTES,
        new::cratestack_schema::SCHEMA_SHA256_BYTES
    );
}
