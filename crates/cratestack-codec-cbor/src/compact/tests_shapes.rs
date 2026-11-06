//! Page, lists at the root, tags, and indefinite-length containers.

use cratestack_core::compact::KeyNode;
use serde_json::json;

use super::tests_support::{T, cbor, i, n, tables, v};
use super::{cbor_from_compact, cbor_to_compact};

fn page_node() -> KeyNode {
    KeyNode::Page(Box::new(KeyNode::Struct(2)))
}

fn page(items: Vec<T>) -> T {
    T::Map(vec![
        n("items", T::Arr(items)),
        n("totalCount", v(json!(2))),
        n(
            "pageInfo",
            T::Map(vec![
                n("limit", v(json!(10))),
                n("hasNextPage", v(json!(false))),
            ]),
        ),
    ])
}

#[test]
fn page_keeps_its_wrapper_named_and_maps_only_the_items() {
    let tables = tables();
    let named = page(vec![
        T::Map(vec![n("sku", v(json!("A"))), n("qty", v(json!(1)))]),
        T::Map(vec![n("sku", v(json!("B")))]),
    ]);
    let want = page(vec![
        T::Map(vec![i(0, v(json!("A"))), i(1, v(json!(1)))]),
        T::Map(vec![i(0, v(json!("B")))]),
    ]);
    let compact = cbor_to_compact(&tables, &page_node(), &cbor(&named)).unwrap();
    assert_eq!(compact, cbor(&want));
    assert_eq!(
        cbor_from_compact(&tables, &page_node(), &compact).unwrap(),
        cbor(&named)
    );
}

#[test]
fn a_root_list_maps_each_element() {
    let tables = tables();
    let node = KeyNode::List(Box::new(KeyNode::Struct(2)));
    let rows = T::Arr(vec![T::Map(vec![n("sku", v(json!("A")))]), T::Arr(vec![])]);
    let want = T::Arr(vec![T::Map(vec![i(0, v(json!("A")))]), T::Arr(vec![])]);
    let compact = cbor_to_compact(&tables, &node, &cbor(&rows)).unwrap();
    assert_eq!(compact, cbor(&want));
    assert_eq!(
        cbor_from_compact(&tables, &node, &compact).unwrap(),
        cbor(&rows)
    );
}

#[test]
fn tags_are_transparent() {
    let tables = tables();
    // 0xc1 (tag 1) wrapping {"sku": 1}
    let named = [&[0xc1_u8][..], &cbor(&T::Map(vec![n("sku", v(json!(1)))]))].concat();
    let node = KeyNode::Struct(2);
    let compact = cbor_to_compact(&tables, &node, &named).unwrap();
    assert_eq!(
        compact,
        [&[0xc1_u8][..], &cbor(&T::Map(vec![i(0, v(json!(1)))]))].concat()
    );
    assert_eq!(cbor_from_compact(&tables, &node, &compact).unwrap(), named);
}

#[test]
fn indefinite_length_containers_round_trip() {
    let tables = tables();
    let node = KeyNode::List(Box::new(KeyNode::Struct(2)));
    // [_ {_ "sku": "A", "zzz": 1}, {"qty": 2}]
    let named = [
        &[0x9f_u8, 0xbf][..],
        &cbor(&json!("sku")),
        &cbor(&json!("A")),
        &cbor(&json!("zzz")),
        &cbor(&json!(1)),
        &[0xff],
        &cbor(&T::Map(vec![n("qty", v(json!(2)))])),
        &[0xff],
    ]
    .concat();
    let want = [
        &[0x9f_u8, 0xbf][..],
        &cbor(&json!(0)),
        &cbor(&json!("A")),
        &[0xff],
        &cbor(&T::Map(vec![i(1, v(json!(2)))])),
        &[0xff],
    ]
    .concat();
    assert_eq!(cbor_to_compact(&tables, &node, &named).unwrap(), want);
    let restored = [
        &[0x9f_u8, 0xbf][..],
        &cbor(&json!("sku")),
        &cbor(&json!("A")),
        &[0xff],
        &cbor(&T::Map(vec![n("qty", v(json!(2)))])),
        &[0xff],
    ]
    .concat();
    assert_eq!(cbor_from_compact(&tables, &node, &want).unwrap(), restored);
}

#[test]
fn absurd_nesting_is_refused_not_overflowed() {
    use cratestack_core::compact::{KeyStruct, KeyTables};
    // A type that contains itself: the bytes decide how deep the walk goes.
    let tables = KeyTables {
        types: vec![KeyStruct {
            fields: vec![("next".to_owned(), KeyNode::Struct(0))],
        }],
        ops: vec![],
    };
    let nest = |depth: usize| {
        let mut body = vec![0xf6_u8];
        for _ in 0..depth {
            let mut level = vec![0xa1_u8];
            level.extend(cbor(&json!("next")));
            level.extend(body);
            body = level;
        }
        body
    };
    assert!(cbor_to_compact(&tables, &KeyNode::Struct(0), &nest(100)).is_ok());
    assert!(cbor_to_compact(&tables, &KeyNode::Struct(0), &nest(400)).is_err());
}
