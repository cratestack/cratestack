use std::path::Path;

use cratestack_core::{digest_hex, op_contract_digests};

use super::ops_report;
use super::tests::{RPC, op, report};

#[test]
fn every_op_carries_the_digest_contract_digest_prints() {
    let schema = cratestack_parser::parse_schema(RPC).unwrap();
    let doc = report(RPC);
    let table = op_contract_digests(&schema);
    assert_eq!(doc["ops"].as_array().unwrap().len(), table.len());
    for (key, digest) in table {
        assert_eq!(
            op(&doc, &key)["contract_digest"],
            digest_hex(&digest),
            "{key}"
        );
    }
}

#[test]
fn the_table_has_a_row_per_op() {
    let schema = cratestack_parser::parse_schema(RPC).unwrap();
    let table = ops_report(&schema, Path::new("s.cstack"), false).unwrap();
    assert!(table.starts_with("OP"), "{table}");
    let either = table
        .lines()
        .find(|l| l.starts_with("procedure.either"))
        .unwrap();
    assert!(
        either.contains("required") && either.contains("a,b"),
        "{either}"
    );
    let open = table
        .lines()
        .find(|l| l.starts_with("procedure.open"))
        .unwrap();
    assert!(open.contains("no"), "{open}");
    assert!(table.contains("unanalysed:"), "{table}");
    assert!(table.contains("denied for every caller"), "{table}");
}
