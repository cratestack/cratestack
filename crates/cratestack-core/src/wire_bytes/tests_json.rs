//! JSON identity: every field shape serializes exactly as plain serde does
//! under a human-readable serializer (cratestack#1425).

use serde::Serialize;

use super::tests::{List, Nullable, PatchList, PatchNullable, Required, json};

/// Plain-serde mirrors, for the JSON identity check.
#[derive(Serialize)]
struct PlainRequired {
    data: Vec<u8>,
}
#[derive(Serialize)]
struct PlainNullable {
    data: Option<Vec<u8>>,
}
#[derive(Serialize)]
struct PlainList {
    data: Vec<Vec<u8>>,
}
#[derive(Serialize)]
struct PlainPatchNullable {
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Option<Vec<u8>>>,
}
#[derive(Serialize)]
struct PlainPatchList {
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Vec<Vec<u8>>>,
}

#[test]
fn json_is_identical_to_plain_serde_for_all_six_shapes() {
    let bytes = vec![0u8, 1, 127, 128, 255];
    let list = vec![vec![1u8, 2], vec![], vec![255]];
    assert_eq!(
        json(&Required {
            data: bytes.clone()
        }),
        json(&PlainRequired {
            data: bytes.clone()
        })
    );
    for data in [None, Some(bytes.clone())] {
        assert_eq!(
            json(&Nullable { data: data.clone() }),
            json(&PlainNullable { data })
        );
    }
    assert_eq!(
        json(&List { data: list.clone() }),
        json(&PlainList { data: list.clone() })
    );
    for data in [None, Some(None), Some(Some(bytes.clone()))] {
        assert_eq!(
            json(&PatchNullable { data: data.clone() }),
            json(&PlainPatchNullable { data })
        );
    }
    for data in [None, Some(list.clone())] {
        assert_eq!(
            json(&PatchList { data: data.clone() }),
            json(&PlainPatchList { data })
        );
    }
    // `serde_json::to_value` (audit, outbox, cursors, MCP) is human-readable too.
    let value = serde_json::to_value(Required {
        data: bytes.clone(),
    })
    .unwrap();
    assert_eq!(value, serde_json::json!({ "data": [0, 1, 127, 128, 255] }));
}
