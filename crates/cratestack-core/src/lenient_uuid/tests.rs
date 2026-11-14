//! Wire-level tests for [`super`]. CBOR assertions go through
//! `minicbor-serde`'s unconfigured deserializer, which is exactly what
//! `CborCodec::decode` uses, so bytes accepted here are accepted by the
//! real server decode path.

use serde::Deserialize;

use super::*;
use crate::FieldFilterInput;

const TEXT: &str = "67e55044-10b1-426f-9247-bb680e5fe0c8";

fn id() -> Uuid {
    TEXT.parse().unwrap()
}

/// A one-entry CBOR map `{"p": <payload>}`.
fn cbor(payload: impl FnOnce(&mut minicbor::Encoder<&mut Vec<u8>>)) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut enc = minicbor::Encoder::new(&mut buf);
    enc.map(1).unwrap().str("p").unwrap();
    payload(&mut enc);
    buf
}

fn text(s: &str) -> Vec<u8> {
    cbor(|e| {
        e.str(s).unwrap();
    })
}

fn bytes(b: &[u8]) -> Vec<u8> {
    cbor(|e| {
        e.bytes(b).unwrap();
    })
}

#[derive(Debug, Deserialize, PartialEq)]
struct Required {
    #[serde(deserialize_with = "deserialize_uuid")]
    p: Uuid,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Optional {
    #[serde(default, deserialize_with = "deserialize_optional_uuid")]
    p: Option<Uuid>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct List {
    #[serde(deserialize_with = "deserialize_uuid_list")]
    p: Vec<Uuid>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct OptionalList {
    #[serde(default, deserialize_with = "deserialize_optional_uuid_list")]
    p: Option<Vec<Uuid>>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Double {
    #[serde(default, deserialize_with = "deserialize_double_option_uuid")]
    p: Option<Option<Uuid>>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Filter {
    #[serde(default, deserialize_with = "deserialize_optional_uuid_filter")]
    p: Option<FieldFilterInput<Uuid>>,
}

fn from_cbor<'a, T: Deserialize<'a>>(b: &'a [u8]) -> Result<T, minicbor_serde::error::DecodeError> {
    minicbor_serde::from_slice(b)
}

#[test]
fn required_accepts_cbor_text_and_cbor_bytes_and_refuses_bad_input() {
    assert_eq!(from_cbor::<Required>(&text(TEXT)).unwrap().p, id());
    assert_eq!(
        from_cbor::<Required>(&bytes(id().as_bytes())).unwrap().p,
        id()
    );
    let short = from_cbor::<Required>(&bytes(&[1; 15]))
        .unwrap_err()
        .to_string();
    assert!(short.contains("UUID parsing failed"), "{short}");
    let bad = from_cbor::<Required>(&text("not-a-uuid"))
        .unwrap_err()
        .to_string();
    assert!(bad.contains("UUID parsing failed"), "{bad}");
}

#[test]
fn required_json_is_unchanged() {
    let ok: Required = serde_json::from_str(&format!(r#"{{"p":"{TEXT}"}}"#)).unwrap();
    assert_eq!(ok.p, id());
    assert!(serde_json::from_str::<Required>(r#"{"p":[1,2,3]}"#).is_err());
    let lenient = serde_json::from_str::<Required>(r#"{"p":"nope"}"#)
        .unwrap_err()
        .to_string();
    #[derive(Debug, Deserialize)]
    struct Strict {
        #[allow(dead_code)]
        p: Uuid,
    }
    let strict = serde_json::from_str::<Strict>(r#"{"p":"nope"}"#)
        .unwrap_err()
        .to_string();
    assert_eq!(lenient, strict, "JSON error text must match uuid's own");
}

#[test]
fn optional_shapes_default_and_accept_both_forms() {
    let missing = cbor_empty_map();
    assert_eq!(from_cbor::<Optional>(&missing).unwrap().p, None);
    assert_eq!(from_cbor::<Optional>(&text(TEXT)).unwrap().p, Some(id()));
    assert_eq!(
        from_cbor::<Optional>(&bytes(id().as_bytes())).unwrap().p,
        Some(id())
    );
    let null = cbor(|e| {
        e.null().unwrap();
    });
    assert_eq!(from_cbor::<Optional>(&null).unwrap().p, None);
    assert_eq!(from_cbor::<OptionalList>(&missing).unwrap().p, None);
    assert_eq!(from_cbor::<Double>(&missing).unwrap().p, None);
    assert_eq!(from_cbor::<Double>(&null).unwrap().p, Some(None));
    assert_eq!(
        from_cbor::<Double>(&text(TEXT)).unwrap().p,
        Some(Some(id()))
    );
    assert_eq!(from_cbor::<Filter>(&missing).unwrap().p, None);
}

fn cbor_empty_map() -> Vec<u8> {
    vec![0xa0]
}

#[test]
fn lists_accept_mixed_forms_per_element() {
    let mixed = cbor(|e| {
        e.array(2)
            .unwrap()
            .str(TEXT)
            .unwrap()
            .bytes(id().as_bytes())
            .unwrap();
    });
    assert_eq!(from_cbor::<List>(&mixed).unwrap().p, vec![id(), id()]);
    assert_eq!(
        from_cbor::<OptionalList>(&mixed).unwrap().p,
        Some(vec![id(), id()])
    );
}

#[test]
fn filter_operands_accept_both_forms() {
    let body = cbor(|e| {
        e.map(3).unwrap();
        e.str("eq").unwrap().str(TEXT).unwrap();
        e.str("in")
            .unwrap()
            .array(2)
            .unwrap()
            .str(TEXT)
            .unwrap()
            .bytes(id().as_bytes())
            .unwrap();
        e.str("isNull").unwrap().bool(false).unwrap();
    });
    let filter = from_cbor::<Filter>(&body).unwrap().p.unwrap();
    assert_eq!(filter.eq, Some(id()));
    assert_eq!(filter.in_, Some(vec![id(), id()]));
    assert_eq!(filter.is_null, Some(false));
}

#[test]
fn map_values_converts_every_operand() {
    let f = FieldFilterInput {
        eq: Some(1),
        ne: Some(2),
        in_: Some(vec![3, 4]),
        lt: Some(5),
        lte: Some(6),
        gt: Some(7),
        gte: Some(8),
        contains: Some("c".into()),
        starts_with: Some("s".into()),
        is_null: Some(true),
    };
    let g = f.map_values(|n| n * 10);
    assert_eq!(g.eq, Some(10));
    assert_eq!(g.in_, Some(vec![30, 40]));
    assert_eq!(g.gte, Some(80));
    assert_eq!(g.contains.as_deref(), Some("c"));
    assert_eq!(g.is_null, Some(true));
}
