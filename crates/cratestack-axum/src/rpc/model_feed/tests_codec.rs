//! The feed encodes with the binding's codec (cratestack#1360): a `Uuid` and
//! bytes are byte strings under CBOR, and under JSON the frame is exactly the
//! `serde_json::to_value` it replaced.

use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_core::CratestackError;
use cratestack_core::rpc::RpcFrameValue;
use futures_util::StreamExt;
use futures_util::stream;
use serde::Serialize;

use super::model_feed;
use crate::rpc::procedure_subscription::procedure_feed;

#[derive(Serialize)]
struct Record {
    id: uuid::Uuid,
    data: cratestack_core::Value,
}

fn record() -> Record {
    Record {
        id: uuid::Uuid::from_u128(7),
        data: cratestack_core::Value::Bytes(vec![9, 8]),
    }
}

#[tokio::test]
async fn cbor_items_keep_uuids_and_bytes_binary() {
    let route = |record: Record| async move { Ok::<_, CratestackError>(Some(record)) };
    let out: Vec<_> = model_feed(stream::iter([Ok(record())]), CborCodec, route)
        .collect()
        .await;
    let Some(Ok(RpcFrameValue::Object(fields))) = out.first() else {
        panic!("expected an object item, got {out:?}");
    };
    assert_eq!(
        fields["id"],
        RpcFrameValue::Bytes(7u128.to_be_bytes().to_vec())
    );
    assert_eq!(fields["data"], RpcFrameValue::Bytes(vec![9, 8]));

    let procedure: Vec<_> = procedure_feed(Box::pin(stream::iter([Ok(record())])), CborCodec)
        .collect()
        .await;
    assert_eq!(procedure[0].as_ref().unwrap(), out[0].as_ref().unwrap());
}

#[tokio::test]
async fn json_items_equal_to_value() {
    let route = |record: Record| async move { Ok::<_, CratestackError>(Some(record)) };
    let out: Vec<_> = model_feed(stream::iter([Ok(record())]), JsonCodec, route)
        .collect()
        .await;
    let expected = serde_json::to_value(record()).unwrap();
    assert_eq!(
        serde_json::to_string(out[0].as_ref().unwrap()).unwrap(),
        serde_json::to_string(&expected).unwrap()
    );
}
