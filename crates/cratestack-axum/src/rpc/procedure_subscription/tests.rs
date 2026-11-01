use cratestack_core::CratestackError;
use futures_util::StreamExt;
use futures_util::stream;
use serde::Deserialize;
use serde_json::json;

use super::{procedure_feed, procedure_input_query, procedure_query_args};

#[derive(Debug, Deserialize, PartialEq)]
struct Args {
    args: Room,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Room {
    #[serde(rename = "roomId")]
    room_id: String,
}

#[test]
fn input_and_query_meet_as_the_same_args() {
    let query = procedure_input_query(json!({"args": {"roomId": "r1"}}))
        .unwrap()
        .expect("a query");
    let args: Args = procedure_query_args(Some(&query)).unwrap();
    assert_eq!(args.args.room_id, "r1");
    assert_eq!(procedure_input_query(json!({})).unwrap(), None);
    assert_eq!(procedure_input_query(json!(null)).unwrap(), None);
    assert!(procedure_input_query(json!([1])).is_err());
}

#[test]
fn a_query_argument_must_be_json() {
    let error = procedure_query_args::<Args>(Some("args=r1")).unwrap_err();
    assert!(error.to_string().contains("must be JSON"), "{error}");
    let error = procedure_query_args::<Args>(Some("args={}&args={}")).unwrap_err();
    assert!(error.to_string().contains("given twice"), "{error}");
    let error = procedure_query_args::<Args>(None).unwrap_err();
    assert!(
        error.to_string().contains("invalid subscription arguments"),
        "{error}"
    );
}

#[tokio::test]
async fn a_feed_ends_after_its_first_error_and_on_its_own() {
    let items = procedure_feed(Box::pin(stream::iter([
        Ok(1),
        Err(CratestackError::Forbidden("gone".to_owned())),
        Ok(2),
    ])));
    let items: Vec<_> = items.collect().await;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].as_ref().unwrap(), &json!(1));
    assert!(items[1].is_err());
    let ended: Vec<_> = procedure_feed(Box::pin(stream::iter([Ok::<_, CratestackError>("a")])))
        .collect()
        .await;
    assert_eq!(ended.len(), 1);
}
