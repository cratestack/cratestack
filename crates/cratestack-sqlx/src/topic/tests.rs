use std::sync::Arc;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use super::Topic;
use crate::backplane::InProcessBackplane;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Message {
    body: String,
}

#[tokio::test]
async fn a_topic_delivers_under_its_key_only() {
    let backplane = Arc::new(InProcessBackplane::new());
    let room = |key: &str| Topic::<Message>::new(backplane.clone(), "RoomMessage", key);
    assert_eq!(room("r1").channel(), "cratestack.topic.RoomMessage.r1");
    let mut r1 = room("r1").subscribe().await.unwrap();
    let mut r2 = room("r2").subscribe().await.unwrap();
    let hello = Message {
        body: "hello".to_owned(),
    };
    room("r1").publish(&hello).await.unwrap();
    assert_eq!(r1.next().await.unwrap().unwrap(), hello);
    let nothing = tokio::time::timeout(std::time::Duration::from_millis(50), r2.next()).await;
    assert!(nothing.is_err(), "r2 received r1's message");
}
