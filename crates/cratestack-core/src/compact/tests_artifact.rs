//! The artifact: round trip, determinism, and refusals.

use super::tests_support::shop;
use super::{CompactError, KeyNode, KeyStruct, KeyTables, OpKeys};

#[test]
fn artifact_round_trips_and_is_deterministic() {
    for transport in ["rest", "rpc"] {
        let tables = KeyTables::from_schema(&shop(transport, false));
        let bytes = tables.to_bytes();
        assert_eq!(
            bytes,
            KeyTables::from_schema(&shop(transport, true)).to_bytes()
        );
        assert_eq!(KeyTables::from_bytes(&bytes).unwrap(), tables);
        // A leading format number, then structs and ops.
        assert_eq!(&bytes[..2], &[0x83, 0x01]);
    }
}

#[test]
fn empty_tables_round_trip() {
    let tables = KeyTables::default();
    assert_eq!(KeyTables::from_bytes(&tables.to_bytes()).unwrap(), tables);
}

fn tiny() -> KeyTables {
    KeyTables {
        types: vec![KeyStruct {
            fields: vec![
                ("a".to_owned(), KeyNode::Struct(0)),
                ("b".to_owned(), KeyNode::Opaque),
            ],
        }],
        ops: vec![OpKeys {
            op: "procedure.x".to_owned(),
            digest: [7; 32],
            request: KeyNode::Page(Box::new(KeyNode::List(Box::new(KeyNode::Struct(0))))),
            response: KeyNode::Opaque,
        }],
    }
}

#[test]
fn recursive_types_survive_the_artifact() {
    let tables = tiny();
    assert_eq!(KeyTables::from_bytes(&tables.to_bytes()).unwrap(), tables);
}

#[test]
fn artifact_reader_refuses_bad_input() {
    let good = tiny().to_bytes();
    let refused =
        |bytes: &[u8]| matches!(KeyTables::from_bytes(bytes), Err(CompactError::Artifact(_)));
    assert!(refused(&[]));
    assert!(refused(&good[..good.len() - 1]));
    let mut trailing = good.clone();
    trailing.push(0);
    assert!(refused(&trailing));
    let mut other_format = good.clone();
    other_format[1] = 2;
    assert!(refused(&other_format));
    let mut dangling = tiny();
    dangling.ops[0].response = KeyNode::Struct(5);
    assert!(refused(&dangling.to_bytes()));
    let mut unsorted = tiny();
    unsorted.ops.push(unsorted.ops[0].clone());
    assert!(refused(&unsorted.to_bytes()));
}
