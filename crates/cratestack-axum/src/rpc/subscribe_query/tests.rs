use cratestack_core::{CratestackError, ModelEventKind, SubscriptionStream};
use futures_util::StreamExt;
use futures_util::stream;

use super::{project_event_fields, split_subscribe_query, subscribe_input_query};

#[test]
fn operations_are_split_from_the_filter() {
    let query = split_subscribe_query(Some("status=open&operations=created,deleted")).unwrap();
    assert_eq!(
        query.operations,
        Some(vec![ModelEventKind::Created, ModelEventKind::Deleted])
    );
    assert_eq!(query.rest.as_deref(), Some("status=open"));
    assert_eq!(split_subscribe_query(None).unwrap(), Default::default());
}

#[test]
fn list_only_parameters_and_unknown_operations_are_refused() {
    for raw in [
        "limit=10",
        "sort=name",
        "includeFields[author]=id",
        "operations=renamed",
    ] {
        assert!(
            matches!(
                split_subscribe_query(Some(raw)),
                Err(CratestackError::BadRequest(_))
            ),
            "{raw} should be refused"
        );
    }
}

#[test]
fn a_frame_input_becomes_the_query() {
    assert_eq!(
        subscribe_input_query(serde_json::Value::Null).unwrap(),
        None
    );
    assert_eq!(subscribe_input_query(serde_json::json!({})).unwrap(), None);
    assert_eq!(
        subscribe_input_query(
            serde_json::json!({"where": "status = 'open'", "operations": ["created"]})
        )
        .unwrap()
        .as_deref(),
        Some("where=status+%3D+%27open%27&operations=created")
    );
    assert!(subscribe_input_query(serde_json::json!({"limit": 1})).is_err());
}

#[tokio::test]
async fn fields_project_each_record() {
    let items: SubscriptionStream<serde_json::Value> = Box::pin(stream::iter([Ok(
        serde_json::json!({"model": "Note", "data": {"id": 1, "body": "x", "secret": 2}}),
    )]));
    let out: Vec<_> = project_event_fields(items, Some(vec!["id".to_owned(), "body".to_owned()]))
        .collect()
        .await;
    assert_eq!(
        out[0].as_ref().unwrap(),
        &serde_json::json!({"model": "Note", "data": {"id": 1, "body": "x"}})
    );
}
