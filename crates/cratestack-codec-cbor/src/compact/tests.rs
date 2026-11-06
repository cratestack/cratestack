//! Rewrite behaviour on a hand-built table.

use cratestack_core::compact::KeyNode;
use serde_json::{Value, json};

use super::tests_support::{T, cbor, i, n, tables, v};
use super::{cbor_from_compact, cbor_to_compact};

const ORDER: KeyNode = KeyNode::Struct(0);

fn meta() -> T {
    v(json!({"id": 7, "nested": {"customer": null}}))
}

fn named_order() -> T {
    T::Map(vec![
        n("id", v(json!("0b7e4d21"))),
        n("total", v(json!("24.90"))),
        n(
            "customer",
            T::Map(vec![
                n("name", v(json!("Ada"))),
                n("orders", T::Arr(vec![T::Map(vec![n("id", v(json!("x")))])])),
            ]),
        ),
        n(
            "lines",
            T::Arr(vec![
                T::Map(vec![n("sku", v(json!("A-1"))), n("qty", v(json!(2)))]),
                T::Map(vec![n("sku", v(json!("B-2"))), n("qty", v(json!(1)))]),
            ]),
        ),
        n("meta", meta()),
    ])
}

fn compact_order() -> T {
    T::Map(vec![
        i(0, v(json!("0b7e4d21"))),
        i(1, v(json!("24.90"))),
        i(
            2,
            T::Map(vec![
                i(0, v(json!("Ada"))),
                i(1, T::Arr(vec![T::Map(vec![i(0, v(json!("x")))])])),
            ]),
        ),
        i(
            3,
            T::Arr(vec![
                T::Map(vec![i(0, v(json!("A-1"))), i(1, v(json!(2)))]),
                T::Map(vec![i(0, v(json!("B-2"))), i(1, v(json!(1)))]),
            ]),
        ),
        // Json is opaque: its own keys are never numbered.
        i(4, meta()),
    ])
}

#[test]
fn field_names_become_indices_through_nested_types_lists_and_recursion() {
    let compact = cbor_to_compact(&tables(), &ORDER, &cbor(&named_order())).unwrap();
    assert_eq!(compact, cbor(&compact_order()));
    assert!(compact.len() < cbor(&named_order()).len());
}

#[test]
fn compact_to_named_restores_the_exact_bytes() {
    let tables = tables();
    let named = cbor(&named_order());
    let compact = cbor_to_compact(&tables, &ORDER, &named).unwrap();
    assert_eq!(cbor_from_compact(&tables, &ORDER, &compact).unwrap(), named);
}

#[test]
fn decode_accepts_a_mixed_map_and_rewrites_the_values_of_text_keys() {
    let mixed = T::Map(vec![
        i(0, v(json!("a"))),
        n("total", v(json!("1.00"))),
        // A text-keyed field whose value is itself compact.
        n(
            "lines",
            T::Arr(vec![T::Map(vec![
                i(0, v(json!("S"))),
                n("qty", v(json!(3))),
            ])]),
        ),
        // Not in the table: passes through.
        n("extra", v(json!([1, 2]))),
    ]);
    let want = T::Map(vec![
        n("id", v(json!("a"))),
        n("total", v(json!("1.00"))),
        n(
            "lines",
            T::Arr(vec![T::Map(vec![
                n("sku", v(json!("S"))),
                n("qty", v(json!(3))),
            ])]),
        ),
        n("extra", v(json!([1, 2]))),
    ]);
    assert_eq!(
        cbor_from_compact(&tables(), &ORDER, &cbor(&mixed)).unwrap(),
        cbor(&want)
    );
}

#[test]
fn an_unknown_integer_key_on_decode_is_an_error() {
    for key in [5_u64, 99, 1 << 40] {
        let body = cbor(&T::Map(vec![i(key, v(json!(1)))]));
        assert!(
            cbor_from_compact(&tables(), &ORDER, &body).is_err(),
            "{key}"
        );
    }
    // The same inside a nested list.
    let nested = T::Map(vec![i(3, T::Arr(vec![T::Map(vec![i(2, v(json!(1)))])]))]);
    assert!(cbor_from_compact(&tables(), &ORDER, &cbor(&nested)).is_err());
    let negative = cbor(&T::V(json!({"x": 1}))); // a text key is fine ...
    assert!(cbor_from_compact(&tables(), &ORDER, &negative).is_ok());
    assert!(cbor_from_compact(&tables(), &ORDER, &[0xa1, 0x20, 0x01]).is_err()); // ... {-1: 1} is not
}

#[test]
fn encode_drops_fields_the_table_lacks() {
    let body = T::Map(vec![
        n("id", v(json!("a"))),
        n(
            "addedByANewerServer",
            T::Map(vec![n("deep", v(json!([1, {"a": 2}])))]),
        ),
        n("total", v(json!("1.00"))),
    ]);
    let want = T::Map(vec![i(0, v(json!("a"))), i(1, v(json!("1.00")))]);
    assert_eq!(
        cbor_to_compact(&tables(), &ORDER, &cbor(&body)).unwrap(),
        cbor(&want)
    );
}

#[test]
fn optionals_nulls_and_non_containers_are_copied() {
    let tables = tables();
    let body = T::Map(vec![
        n("customer", v(Value::Null)),
        n("lines", v(Value::Null)),
    ]);
    let compact = cbor_to_compact(&tables, &ORDER, &cbor(&body)).unwrap();
    assert_eq!(
        compact,
        cbor(&T::Map(vec![i(2, v(Value::Null)), i(3, v(Value::Null))]))
    );
    assert_eq!(
        cbor_from_compact(&tables, &ORDER, &compact).unwrap(),
        cbor(&body)
    );
    // A struct node over a scalar, or a list node over a map, changes nothing.
    for scalar in [cbor(&json!(5)), cbor(&json!("text")), cbor(&Value::Null)] {
        assert_eq!(cbor_to_compact(&tables, &ORDER, &scalar).unwrap(), scalar);
    }
    let map = cbor(&named_order());
    let list = KeyNode::List(Box::new(ORDER));
    assert_eq!(cbor_to_compact(&tables, &list, &map).unwrap(), map);
}

#[test]
fn trailing_or_truncated_bytes_are_errors() {
    let tables = tables();
    let mut bytes = cbor(&named_order());
    bytes.push(0);
    assert!(cbor_to_compact(&tables, &ORDER, &bytes).is_err());
    bytes.truncate(bytes.len() - 6);
    assert!(cbor_to_compact(&tables, &ORDER, &bytes).is_err());
    assert!(cbor_from_compact(&tables, &ORDER, &[]).is_err());
}
