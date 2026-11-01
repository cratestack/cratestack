use super::*;

#[test]
fn blocks_parse_and_comments_yield_nothing() {
    assert_eq!(
        parse_block("event: ready\ndata: {\"id\":1,\"ready\":{}}\n\n"),
        Some(Event {
            name: "ready".into(),
            data: "{\"id\":1,\"ready\":{}}".into()
        })
    );
    assert_eq!(parse_block(": keepalive\n\n"), None);
    assert_eq!(parse_block("data: a\ndata: b\n\n").unwrap().data, "a\nb");
}

#[tokio::test]
async fn events_split_across_chunks() {
    let chunks = vec![
        Ok(Bytes::from_static(b"event: ready\ndata: {}\n")),
        Ok(Bytes::from_static(b"\n: keepalive\n\nevent: message\nda")),
        Ok(Bytes::from_static(b"ta: {\"next\":1}\n\n")),
    ];
    let events: Vec<_> = events(futures_util::stream::iter(chunks)).collect().await;
    let names: Vec<_> = events
        .iter()
        .map(|e| e.as_ref().unwrap().name.as_str())
        .collect();
    assert_eq!(names, ["ready", "message"]);
    assert_eq!(
        next_of::<i64>(&events[1].as_ref().unwrap().data).unwrap(),
        1
    );
}

#[tokio::test]
async fn utf8_and_crlf_split_across_chunks() {
    let chunks = vec![
        Ok(Bytes::from_static(b"event: message\r\ndata: {\"next\":\"caf\xc3")),
        Ok(Bytes::from_static(b"\xa9\"}\r\n\r")),
        Ok(Bytes::from_static(b"\n")),
    ];
    let events: Vec<_> = events(futures_util::stream::iter(chunks)).collect().await;
    assert_eq!(events.len(), 1);
    let event = events[0].as_ref().unwrap();
    assert_eq!(event.name, "message");
    assert_eq!(next_of::<String>(&event.data).unwrap(), "café");
}

#[tokio::test]
async fn invalid_utf8_in_a_block_is_an_error() {
    let chunks = vec![Ok(Bytes::from_static(b"data: \xff\n\n"))];
    let events: Vec<_> = events(futures_util::stream::iter(chunks)).collect().await;
    assert!(matches!(events[0], Err(ClientError::InvalidResponse(_))));
}
