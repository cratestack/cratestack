//! ADR 0019 PR B (B11): the wire contract of `BigInt` on every shape the macros
//! generate, decoded and encoded by the real server codecs. DB-free, so it can
//! never silently skip (modelled on `bytes_wire_shapes.rs`).
//!
//! The contract under test is D2 and D3:
//! - a `BigInt` is a canonical decimal string (JSON string, CBOR text string,
//!   major type 3) on every codec and on every shape, including the shapes a
//!   `serde(with = ...)` on a raw `i64` would silently miss: the model, the
//!   create and patch-wrapped update inputs, a `type` block (required, optional
//!   and list), procedure arguments and `<Model>Where`;
//! - only that form is accepted: a JSON number, a CBOR integer (major type 0 or
//!   1), a CBOR bignum (tag 2 or 3), `+5`, `007`, `-0`, `" 1"` and anything
//!   outside `i64` are refused, on both codecs, with the field named in the
//!   error's `detail()` (decision D-MSG; the public message stays generic);
//! - `<Model>Where` filters on a `BigInt` field for real. A `Where` that dropped
//!   the field would still decode (unknown keys are ignored), so the test
//!   asserts on the filters and on the SQL it produces (ADR 0019 risk 3).

use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{BigInt, CratestackCodec, CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use serde::de::DeserializeOwned;

include_server_schema!("tests/fixtures/bigint_wire_shapes.cstack", db = Postgres);

mod bigint_support;

use bigint_support::{
    ABOVE_2_53, BOUNDARY, I64_MAX, I64_MIN, Wire, cbor_list, cbor_map, cbor_text,
};
use cratestack_schema::procedures::bi_shape_echo;
use cratestack_schema::{
    BiShapeEnvelope, BiShapeRow, BiShapeRowFindManyInput, BiShapeRowWhere, CreateBiShapeRowInput,
    UpdateBiShapeRowInput, build_bi_shape_row_query_from_find_many,
};
use serde_json::{Value as Json, json};

/// Every value the round trips run at: the three boundary values of the ADR
/// plus the ones a sign or zero handling bug would break.
fn values() -> Vec<(String, i64)> {
    let mut all: Vec<(String, i64)> = BOUNDARY
        .iter()
        .map(|(s, n)| ((*s).to_owned(), *n))
        .collect();
    all.push(("0".to_owned(), 0));
    all.push(("-1".to_owned(), -1));
    all
}

fn decode<T: DeserializeOwned>(wire: Wire, body: &Json) -> T {
    let bytes = wire.encode(body);
    let result: Result<T, CratestackError> = match wire {
        Wire::Json => JsonCodec.decode(&bytes),
        Wire::Cbor => CborCodec.decode(&bytes),
    };
    result.unwrap_or_else(|e| panic!("{wire:?} decode of {body} failed: {e:?}"))
}

fn decode_bytes<T: DeserializeOwned>(wire: Wire, bytes: &[u8]) -> Result<T, CratestackError> {
    match wire {
        Wire::Json => JsonCodec.decode(bytes),
        Wire::Cbor => CborCodec.decode(bytes),
    }
}

// ---------------------------------------------------------------------------
// Accepted forms
// ---------------------------------------------------------------------------

#[test]
fn canonical_strings_decode_to_the_exact_i64_in_every_generated_shape() {
    for wire in Wire::BOTH {
        for (text, n) in values() {
            let expected = BigInt::new(n);
            let create: CreateBiShapeRowInput =
                decode(wire, &json!({"id": text, "amountE8": text, "feeE8": text}));
            assert_eq!(create.id, expected, "{wire:?} create id {text}");
            assert_eq!(create.amountE8, expected, "{wire:?} create amountE8 {text}");
            assert_eq!(create.feeE8, Some(expected), "{wire:?} create feeE8 {text}");

            let model: BiShapeRow =
                decode(wire, &json!({"id": text, "amountE8": text, "feeE8": null}));
            assert_eq!(
                (model.id, model.amountE8, model.feeE8),
                (expected, expected, None)
            );

            let update: UpdateBiShapeRowInput =
                decode(wire, &json!({"amountE8": text, "feeE8": text}));
            assert_eq!(
                update.amountE8,
                Some(expected),
                "{wire:?} update amountE8 {text}"
            );
            assert_eq!(
                update.feeE8,
                Some(Some(expected)),
                "{wire:?} update feeE8 {text}"
            );

            let envelope: BiShapeEnvelope = decode(
                wire,
                &json!({"amountE8": text, "maybeE8": text, "listE8": [text, "0", text]}),
            );
            assert_eq!(envelope.amountE8, expected);
            assert_eq!(envelope.maybeE8, Some(expected));
            assert_eq!(envelope.listE8, vec![expected, BigInt::new(0), expected]);

            let args: bi_shape_echo::Args = decode(
                wire,
                &json!({"amountE8": text, "maybeE8": null, "listE8": [text]}),
            );
            assert_eq!(args.amountE8, expected, "{wire:?} procedure arg {text}");
            assert_eq!(args.maybeE8, None);
            assert_eq!(args.listE8, vec![expected]);
        }
    }
}

#[test]
fn an_explicit_null_clears_an_optional_bigint_and_an_absent_key_leaves_it_alone() {
    for wire in Wire::BOTH {
        let cleared: UpdateBiShapeRowInput = decode(wire, &json!({"feeE8": null}));
        assert_eq!(cleared.feeE8, Some(None), "{wire:?}: null must mean clear");
        let untouched: UpdateBiShapeRowInput = decode(wire, &json!({}));
        assert_eq!(
            untouched.feeE8, None,
            "{wire:?}: absent must mean untouched"
        );
        assert_eq!(untouched.amountE8, None);
    }
}

#[test]
fn every_shape_serializes_a_string_never_a_number() {
    for (text, n) in values() {
        let big = BigInt::new(n);
        let envelope = BiShapeEnvelope {
            amountE8: big,
            maybeE8: Some(big),
            listE8: vec![big, big],
        };
        let as_json = serde_json::to_value(&envelope).expect("serialize");
        assert_eq!(as_json["amountE8"], Json::String(text.clone()));
        assert_eq!(as_json["maybeE8"], Json::String(text.clone()));
        assert_eq!(as_json["listE8"], json!([text, text]));

        let model = BiShapeRow {
            id: big,
            amountE8: big,
            feeE8: Some(big),
        };
        let as_json = serde_json::to_value(&model).expect("serialize");
        for key in ["id", "amountE8", "feeE8"] {
            assert_eq!(as_json[key], Json::String(text.clone()), "model.{key}");
        }

        // And the bytes both codecs write decode back to the same value.
        for wire in Wire::BOTH {
            let bytes = match wire {
                Wire::Json => JsonCodec.encode(&envelope),
                Wire::Cbor => CborCodec.encode(&envelope),
            }
            .expect("encode");
            let back: BiShapeEnvelope = decode_bytes(wire, &bytes).expect("decode");
            assert_eq!(back, envelope, "{wire:?} round trip of {text}");
        }
    }
}

#[test]
fn cbor_writes_a_major_type_3_text_string_byte_for_byte() {
    // The ADR's own numbers: i64::MAX is the initial byte 0x73 (major type 3,
    // length 19) and its 19 ASCII digits, 20 bytes; today's `Int` takes 9.
    assert_eq!(cbor_text(I64_MAX).len(), 20);
    assert_eq!(cbor_text(I64_MAX)[0], 0x73);
    assert_eq!(cbor_text(I64_MIN)[0], 0x74, "i64::MIN is 20 characters");
    assert_eq!(cbor_text(ABOVE_2_53)[0], 0x70, "2^53 + 1 is 16 characters");

    for (text, n) in BOUNDARY {
        // A patch-wrapped update with only `amountE8` touched writes exactly
        // one map entry, so the whole body is predictable.
        let update = UpdateBiShapeRowInput {
            amountE8: Some(BigInt::new(n)),
            ..Default::default()
        };
        let bytes = CborCodec.encode(&update).expect("encode");
        assert_eq!(
            bytes,
            cbor_map(&[("amountE8", cbor_text(text))]),
            "{text}: a BigInt must be a CBOR text string, never an integer or a bignum"
        );
    }
}

#[test]
fn a_value_written_through_serde_json_value_has_the_same_cbor_bytes() {
    // `POST /rpc/batch` carries every frame as `serde_json::Value` and then
    // CBOR (`cratestack-core/src/rpc.rs`). One field must not have two CBOR
    // forms depending on the route (ADR 0019 D2, batch frames).
    for (text, n) in BOUNDARY {
        // One key, so map ordering cannot differ: the bytes must be identical.
        let update = UpdateBiShapeRowInput {
            amountE8: Some(BigInt::new(n)),
            ..Default::default()
        };
        assert_eq!(
            CborCodec.encode(&update).expect("encode typed"),
            CborCodec
                .encode(&serde_json::to_value(&update).expect("to_value"))
                .expect("encode value"),
            "{text}: unary and batch bytes differ"
        );

        // Several keys: `serde_json::Value` orders them, a struct does not, so
        // compare what is on the wire rather than its order.
        let envelope = BiShapeEnvelope {
            amountE8: BigInt::new(n),
            maybeE8: Some(BigInt::new(n)),
            listE8: vec![BigInt::new(n)],
        };
        let direct = CborCodec.encode(&envelope).expect("encode typed");
        let via_value = CborCodec
            .encode(&serde_json::to_value(&envelope).expect("to_value"))
            .expect("encode value");
        let occurrences = |bytes: &[u8]| {
            let needle = cbor_text(text);
            bytes
                .windows(needle.len())
                .filter(|window| *window == needle.as_slice())
                .count()
        };
        assert_eq!(occurrences(&direct), 3, "{text}: typed bytes");
        assert_eq!(occurrences(&via_value), 3, "{text}: via serde_json::Value");
        assert_eq!(direct.len(), via_value.len());

        // The frame's input, re-encoded by the dispatcher, decodes to the value.
        let back: BiShapeEnvelope = decode_bytes(Wire::Cbor, &via_value).expect("decode");
        assert_eq!(back, envelope);
    }
}

// ---------------------------------------------------------------------------
// Refused forms
// ---------------------------------------------------------------------------

/// Strings that are not the canonical form (ADR 0019 D2).
const NON_CANONICAL: &[&str] = &[
    "+5",
    "007",
    "-0",
    " 1",
    "1 ",
    "",
    "-",
    "1.0",
    "1e3",
    "0x10",
    "1_0",
    "9223372036854775808",
    "-9223372036854775809",
    "99999999999999999999",
    "\u{663}",
    "\u{ff11}",
];

/// Raw JSON values that are not a JSON string.
const JSON_NOT_A_STRING: &[&str] = &[
    "5",
    "0",
    "-1",
    "5.0",
    "1e3",
    "9007199254740993",
    "9223372036854775807",
    "-9223372036854775808",
    "9223372036854775808",
    "true",
    "[]",
    "{}",
    r#"["1"]"#,
];

/// Raw CBOR items that are not a text string.
fn cbor_not_a_text_string() -> Vec<(&'static str, Vec<u8>)> {
    let be_i64_max = vec![0x48, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    let mut tag2 = vec![0xc2];
    tag2.extend(&be_i64_max);
    let mut tag3 = vec![0xc3];
    tag3.extend(&be_i64_max);
    vec![
        ("uint 0", vec![0x00]),
        ("uint 5", vec![0x05]),
        ("nint -1", vec![0x20]),
        (
            "uint i64::MAX",
            vec![0x1b, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        ),
        (
            "nint i64::MIN",
            vec![0x3b, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        ),
        (
            "uint 2^53+1",
            vec![0x1b, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01],
        ),
        ("bignum tag 2", tag2),
        ("bignum tag 3", tag3),
        ("float 5.0", vec![0xfb, 0x40, 0x14, 0, 0, 0, 0, 0, 0]),
        ("true", vec![0xf5]),
        ("empty array", vec![0x80]),
        ("empty map", vec![0xa0]),
        ("byte string \"1\"", vec![0x41, 0x31]),
    ]
}

/// Asserts a refusal: a codec error whose `detail()` names `field`, while the
/// public message stays generic (decision D-MSG).
#[track_caller]
fn assert_refused<T: DeserializeOwned + std::fmt::Debug>(
    wire: Wire,
    bytes: &[u8],
    field: &str,
    what: &str,
) {
    match decode_bytes::<T>(wire, bytes) {
        Ok(value) => panic!("{wire:?} accepted {what}: {value:?}"),
        Err(error) => {
            assert!(
                matches!(error, CratestackError::Codec(_)),
                "{wire:?} {what}: expected a codec error (HTTP 400), got {error:?}"
            );
            let detail = error.detail().unwrap_or_default();
            assert!(
                detail.contains(field),
                "{wire:?} {what}: detail must name `{field}`, got `{detail}`"
            );
            assert_eq!(
                error.public_message(),
                "invalid request payload",
                "{wire:?} {what}: the public message stays generic"
            );
        }
    }
}

fn json_body(entries: &[(&str, &str)]) -> Vec<u8> {
    let body = entries
        .iter()
        .map(|(key, raw)| format!(r#""{key}":{raw}"#))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{body}}}").into_bytes()
}

#[test]
fn json_refuses_everything_but_a_canonical_string_and_names_the_field() {
    for raw in JSON_NOT_A_STRING {
        let what = format!("raw JSON `{raw}`");
        assert_refused::<CreateBiShapeRowInput>(
            Wire::Json,
            &json_body(&[("id", r#""1""#), ("amountE8", raw)]),
            "amountE8",
            &what,
        );
        // An optional key must refuse a number too, not read it as absent.
        assert_refused::<CreateBiShapeRowInput>(
            Wire::Json,
            &json_body(&[("id", r#""1""#), ("amountE8", r#""1""#), ("feeE8", raw)]),
            "feeE8",
            &what,
        );
        assert_refused::<BiShapeRow>(
            Wire::Json,
            &json_body(&[("id", raw), ("amountE8", r#""1""#)]),
            "id",
            &what,
        );
        assert_refused::<UpdateBiShapeRowInput>(
            Wire::Json,
            &json_body(&[("amountE8", raw)]),
            "amountE8",
            &what,
        );
        assert_refused::<bi_shape_echo::Args>(
            Wire::Json,
            &json_body(&[("amountE8", raw), ("listE8", "[]")]),
            "amountE8",
            &what,
        );
    }
    // `null` is a refusal only where the key is required.
    assert_refused::<CreateBiShapeRowInput>(
        Wire::Json,
        &json_body(&[("id", r#""1""#), ("amountE8", "null")]),
        "amountE8",
        "null at a required key",
    );
    for text in NON_CANONICAL {
        let raw = format!("{text:?}");
        let what = format!("JSON string {raw}");
        assert_refused::<CreateBiShapeRowInput>(
            Wire::Json,
            &json_body(&[("id", r#""1""#), ("amountE8", &raw)]),
            "amountE8",
            &what,
        );
    }
}

#[test]
fn cbor_refuses_everything_but_a_canonical_text_string_and_names_the_field() {
    for (label, item) in cbor_not_a_text_string() {
        let create = cbor_map(&[("id", cbor_text("1")), ("amountE8", item.clone())]);
        assert_refused::<CreateBiShapeRowInput>(Wire::Cbor, &create, "amountE8", label);
        let optional = cbor_map(&[
            ("id", cbor_text("1")),
            ("amountE8", cbor_text("1")),
            ("feeE8", item.clone()),
        ]);
        assert_refused::<CreateBiShapeRowInput>(Wire::Cbor, &optional, "feeE8", label);
        let key = cbor_map(&[("id", item.clone()), ("amountE8", cbor_text("1"))]);
        assert_refused::<BiShapeRow>(Wire::Cbor, &key, "id", label);
        let update = cbor_map(&[("amountE8", item.clone())]);
        assert_refused::<UpdateBiShapeRowInput>(Wire::Cbor, &update, "amountE8", label);
        let args = cbor_map(&[("amountE8", item.clone()), ("listE8", vec![0x80])]);
        assert_refused::<bi_shape_echo::Args>(Wire::Cbor, &args, "amountE8", label);
    }
    for text in NON_CANONICAL {
        let create = cbor_map(&[("id", cbor_text("1")), ("amountE8", cbor_text(text))]);
        assert_refused::<CreateBiShapeRowInput>(
            Wire::Cbor,
            &create,
            "amountE8",
            &format!("CBOR text {text:?}"),
        );
    }
}

#[test]
fn a_number_inside_a_list_or_a_type_block_is_refused_with_the_field_named() {
    let list_json = json_body(&[
        ("amountE8", r#""1""#),
        ("maybeE8", "null"),
        ("listE8", r#"["1", 2]"#),
    ]);
    assert_refused::<BiShapeEnvelope>(Wire::Json, &list_json, "listE8", "number in a list");
    assert_refused::<bi_shape_echo::Args>(Wire::Json, &list_json, "listE8", "number in a list arg");

    let list_cbor = cbor_map(&[
        ("amountE8", cbor_text("1")),
        ("maybeE8", vec![0xf6]),
        ("listE8", vec![0x82, 0x61, b'1', 0x02]),
    ]);
    assert_refused::<BiShapeEnvelope>(Wire::Cbor, &list_cbor, "listE8", "integer in a list");
    assert_refused::<bi_shape_echo::Args>(
        Wire::Cbor,
        &list_cbor,
        "listE8",
        "integer in a list arg",
    );

    let nested = cbor_map(&[
        ("amountE8", cbor_text("1")),
        ("maybeE8", cbor_list(vec![0x05])),
        ("listE8", vec![0x80]),
    ]);
    assert_refused::<BiShapeEnvelope>(Wire::Cbor, &nested, "maybeE8", "array at an optional key");
}

// ---------------------------------------------------------------------------
// <Model>Where (ADR 0019 risk 3: the key was silently ignored)
// ---------------------------------------------------------------------------

fn lazy_db() -> cratestack_schema::Cratestack {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    cratestack_schema::Cratestack::builder(pool).build()
}

#[tokio::test]
async fn a_where_filter_on_a_bigint_field_is_decoded_and_becomes_sql() {
    let db = lazy_db();
    for wire in Wire::BOTH {
        let where_: BiShapeRowWhere = decode(
            wire,
            &json!({"amountE8": {
                "gt": ABOVE_2_53,
                "lte": I64_MAX,
                "in": [I64_MIN, ABOVE_2_53],
            }}),
        );
        let filter = where_.amountE8.as_ref().unwrap_or_else(|| {
            panic!("{wire:?}: `amountE8` was dropped from BiShapeRowWhere: the key is ignored")
        });
        assert_eq!(filter.gt, Some(BigInt::new(9_007_199_254_740_993)));
        assert_eq!(filter.lte, Some(BigInt::MAX));
        assert_eq!(
            filter.in_,
            Some(vec![BigInt::MIN, BigInt::new(9_007_199_254_740_993)])
        );
        assert_eq!(
            where_.to_filters().len(),
            3,
            "{wire:?}: gt, lte and in must each become a filter"
        );

        let sql = build_bi_shape_row_query_from_find_many(
            &db,
            &BiShapeRowFindManyInput {
                r#where: Some(where_),
                order_by: None,
            },
        )
        .preview_sql();
        assert!(
            sql.contains("WHERE"),
            "{wire:?}: the query has no WHERE: {sql}"
        );
        assert!(
            sql.contains("amount_e8 >"),
            "{wire:?}: no gt on amount_e8: {sql}"
        );
        assert!(
            sql.contains("amount_e8 <="),
            "{wire:?}: no lte on amount_e8: {sql}"
        );
        assert!(
            sql.contains("amount_e8 IN"),
            "{wire:?}: no IN on amount_e8: {sql}"
        );
    }
}

#[test]
fn the_optional_bigint_field_filters_on_is_null_and_eq() {
    let where_: BiShapeRowWhere = decode(
        Wire::Json,
        &json!({"feeE8": {"eq": I64_MIN}, "id": {"ne": "0"}}),
    );
    assert_eq!(where_.feeE8.as_ref().and_then(|f| f.eq), Some(BigInt::MIN));
    assert_eq!(where_.id.as_ref().and_then(|f| f.ne), Some(BigInt::new(0)));
    assert_eq!(where_.to_filters().len(), 2);

    let is_null: BiShapeRowWhere = decode(Wire::Cbor, &json!({"feeE8": {"isNull": true}}));
    assert_eq!(is_null.to_filters().len(), 1);
}

#[test]
fn a_number_in_a_where_filter_is_refused_with_the_field_named() {
    for operator in ["eq", "ne", "lt", "lte", "gt", "gte"] {
        let json = json_body(&[("amountE8", &format!(r#"{{"{operator}":5}}"#))]);
        assert_refused::<BiShapeRowWhere>(
            Wire::Json,
            &json,
            "amountE8",
            &format!("JSON number at `{operator}`"),
        );
        let cbor = cbor_map(&[("amountE8", cbor_map(&[(operator, vec![0x05])]))]);
        assert_refused::<BiShapeRowWhere>(
            Wire::Cbor,
            &cbor,
            "amountE8",
            &format!("CBOR integer at `{operator}`"),
        );
    }
    let in_list = json_body(&[("amountE8", r#"{"in":["1",2]}"#)]);
    assert_refused::<BiShapeRowWhere>(Wire::Json, &in_list, "amountE8", "number inside `in`");
}
