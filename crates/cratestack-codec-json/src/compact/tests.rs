//! Rewrite behaviour on a hand-built table.

use cratestack_core::compact::{KeyNode, KeyStruct, KeyTables};

use super::{json_from_compact, json_to_compact};

/// `Order { id, total, customer: Customer?, lines: Line[], meta: Json }`,
/// `Customer { name, orders: Order[] }` (recursive), `Line { sku, qty }`.
fn tables() -> KeyTables {
    let fields = |list: &[(&str, KeyNode)]| KeyStruct {
        fields: list
            .iter()
            .map(|(n, k)| ((*n).to_owned(), k.clone()))
            .collect(),
    };
    KeyTables {
        types: vec![
            fields(&[
                ("id", KeyNode::Opaque),
                ("total", KeyNode::Opaque),
                ("customer", KeyNode::Struct(1)),
                ("lines", KeyNode::List(Box::new(KeyNode::Struct(2)))),
                ("meta", KeyNode::Opaque),
            ]),
            fields(&[
                ("name", KeyNode::Opaque),
                ("orders", KeyNode::List(Box::new(KeyNode::Struct(0)))),
            ]),
            fields(&[("sku", KeyNode::Opaque), ("qty", KeyNode::Opaque)]),
        ],
        ops: vec![],
    }
}

const ORDER: KeyNode = KeyNode::Struct(0);

fn to(node: &KeyNode, body: &str) -> String {
    String::from_utf8(json_to_compact(&tables(), node, body.as_bytes()).unwrap()).unwrap()
}

fn from(node: &KeyNode, body: &str) -> Result<String, String> {
    json_from_compact(&tables(), node, body.as_bytes())
        .map(|bytes| String::from_utf8(bytes).unwrap())
        .map_err(|e| e.to_string())
}

const NAMED: &str = concat!(
    r#"{"id":"0b7e","total":"24.90","customer":{"name":"Ada","orders":[{"id":"x"}]},"#,
    r#""lines":[{"sku":"A-1","qty":2},{"sku":"B-2","qty":1.50}],"#,
    r#""meta":{"id":7,"big":12345678901234567890123,"nested":{"customer":null}}}"#
);
const COMPACT: &str = concat!(
    r#"{"0":"0b7e","1":"24.90","2":{"0":"Ada","1":[{"0":"x"}]},"#,
    r#""3":[{"0":"A-1","1":2},{"0":"B-2","1":1.50}],"#,
    r#""4":{"id":7,"big":12345678901234567890123,"nested":{"customer":null}}}"#
);

#[test]
fn names_become_decimal_string_keys_and_values_are_copied_verbatim() {
    assert_eq!(to(&ORDER, NAMED), COMPACT);
    assert_eq!(from(&ORDER, COMPACT).unwrap(), NAMED);
}

#[test]
fn decode_accepts_a_mixed_object_and_passes_unknown_names_through() {
    let mixed = r#"{"0":"a","total":"1.00","lines":[{"0":"S","qty":3}],"extra":[1,{"z":2}]}"#;
    let want = r#"{"id":"a","total":"1.00","lines":[{"sku":"S","qty":3}],"extra":[1,{"z":2}]}"#;
    assert_eq!(from(&ORDER, mixed).unwrap(), want);
}

#[test]
fn an_unknown_numeric_key_on_decode_is_an_error() {
    for body in [
        r#"{"5":1}"#,
        r#"{"99":1}"#,
        r#"{"3":[{"2":1}]}"#,
        r#"{"03":1}"#,
    ] {
        assert!(from(&ORDER, body).is_err(), "{body}");
    }
}

#[test]
fn encode_drops_fields_the_table_lacks_and_keeps_commas_valid() {
    let named = r#"{"newFirst":{"a":[1]},"id":"a","newMid":1,"total":"1","newLast":2}"#;
    assert_eq!(to(&ORDER, named), r#"{"0":"a","1":"1"}"#);
    assert_eq!(to(&ORDER, r#"{"newOnly":1}"#), "{}");
}

#[test]
fn page_keeps_wrapper_keys_named_and_maps_the_items() {
    let node = KeyNode::Page(Box::new(KeyNode::Struct(2)));
    let named =
        r#"{"items":[{"sku":"A","qty":1},{"sku":"B"}],"totalCount":2,"pageInfo":{"limit":10}}"#;
    let compact = r#"{"items":[{"0":"A","1":1},{"0":"B"}],"totalCount":2,"pageInfo":{"limit":10}}"#;
    assert_eq!(to(&node, named), compact);
    assert_eq!(from(&node, compact).unwrap(), named);
}

#[test]
fn root_lists_nulls_scalars_and_empty_containers() {
    let list = KeyNode::List(Box::new(KeyNode::Struct(2)));
    assert_eq!(
        to(&list, r#"[{"sku":"A"},null,[],{}]"#),
        r#"[{"0":"A"},null,[],{}]"#
    );
    assert_eq!(to(&ORDER, "null"), "null");
    assert_eq!(to(&ORDER, r#""text""#), r#""text""#);
    assert_eq!(
        to(&ORDER, r#"{"customer":null,"lines":null}"#),
        r#"{"2":null,"3":null}"#
    );
    assert_eq!(to(&list, "[]"), "[]");
    // A list node over an object changes nothing.
    assert_eq!(to(&list, NAMED), NAMED);
}

#[test]
fn escaped_keys_are_matched_by_their_text() {
    // "id" is `id`.
    assert_eq!(to(&ORDER, r#"{"id":1}"#), r#"{"0":1}"#);
    assert_eq!(from(&ORDER, r#"{"0":1}"#).unwrap(), r#"{"id":1}"#);
}

#[test]
fn malformed_json_and_trailing_input_are_errors() {
    for body in [
        "",
        "{",
        r#"{"id":}"#,
        r#"{"id":1,}"#,
        r#"{"id":1} x"#,
        r#"[1"#,
        "{1:2}",
    ] {
        assert!(
            json_to_compact(&tables(), &ORDER, body.as_bytes()).is_err(),
            "{body}"
        );
    }
}

#[test]
fn absurd_nesting_is_refused_not_overflowed() {
    let tables = KeyTables {
        types: vec![KeyStruct {
            fields: vec![("next".to_owned(), KeyNode::Struct(0))],
        }],
        ops: vec![],
    };
    let nest = |depth: usize| format!("{}null{}", r#"{"next":"#.repeat(depth), "}".repeat(depth));
    let run = |depth| json_to_compact(&tables, &KeyNode::Struct(0), nest(depth).as_bytes());
    assert!(run(100).is_ok());
    assert!(run(400).is_err());
}
