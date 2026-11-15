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
    let events: Vec<_> = events(futures_util::stream::iter(chunks), 1 << 20)
        .collect()
        .await;
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
        Ok(Bytes::from_static(
            b"event: message\r\ndata: {\"next\":\"caf\xc3",
        )),
        Ok(Bytes::from_static(b"\xa9\"}\r\n\r")),
        Ok(Bytes::from_static(b"\n")),
    ];
    let events: Vec<_> = events(futures_util::stream::iter(chunks), 1 << 20)
        .collect()
        .await;
    assert_eq!(events.len(), 1);
    let event = events[0].as_ref().unwrap();
    assert_eq!(event.name, "message");
    assert_eq!(next_of::<String>(&event.data).unwrap(), "café");
}

#[tokio::test]
async fn invalid_utf8_in_a_block_is_an_error() {
    let chunks = vec![Ok(Bytes::from_static(b"data: \xff\n\n"))];
    let events: Vec<_> = events(futures_util::stream::iter(chunks), 1 << 20)
        .collect()
        .await;
    assert!(matches!(events[0], Err(ClientError::InvalidResponse(_))));
}

/// An event `size` bytes long, delimiter included, as one `data:` line.
fn event_of(size: usize) -> Vec<u8> {
    let mut event = b"data: ".to_vec();
    event.resize(size - 2, b'x');
    event.extend_from_slice(b"\n\n");
    event
}

async fn collect(chunks: Vec<Vec<u8>>, limit: usize) -> Vec<Result<Event, ClientError>> {
    let chunks: Vec<_> = chunks.into_iter().map(|c| Ok(Bytes::from(c))).collect();
    events(futures_util::stream::iter(chunks), limit)
        .collect()
        .await
}

#[tokio::test]
async fn an_event_exactly_at_the_cap_is_accepted() {
    let out = collect(vec![event_of(100)], 100).await;
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].as_ref().unwrap().data.len(), 92);
}

#[tokio::test]
async fn an_event_one_byte_over_the_cap_is_refused_and_ends_the_stream() {
    // A second, small event follows in the same chunk: it must not arrive.
    let mut body = event_of(101);
    body.extend_from_slice(b"data: after\n\n");
    let out = collect(vec![body], 100).await;
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(matches!(
        out[0],
        Err(ClientError::EventTooLarge { limit: 100 })
    ));
}

#[tokio::test]
async fn an_oversize_event_split_across_chunks_is_refused() {
    let body = event_of(101);
    let (a, b) = body.split_at(60);
    let out = collect(
        vec![a.to_vec(), b.to_vec(), b"data: after\n\n".to_vec()],
        100,
    )
    .await;
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(matches!(out[0], Err(ClientError::EventTooLarge { .. })));
}

/// An endless body that yields to the runtime between chunks, so the
/// `timeout` guards below can fire if the reader never stops (`repeat_with`
/// alone never returns `Pending`, and would hang the test instead).
fn endless_of(chunk: &'static [u8]) -> impl Stream<Item = Result<Bytes, reqwest::Error>> + Unpin {
    Box::pin(futures_util::stream::unfold((), move |()| async move {
        tokio::task::yield_now().await;
        Some((Ok(Bytes::from_static(chunk)), ()))
    }))
}

#[tokio::test]
async fn an_endless_line_with_no_delimiter_ends_with_the_error() {
    let endless = endless_of(&[b'x'; 100]);
    let mut stream = Box::pin(events(endless, 1024));
    let first = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("the reader must stop buffering, not hang");
    assert!(matches!(
        first,
        Some(Err(ClientError::EventTooLarge { limit: 1024 }))
    ));
    assert!(stream.next().await.is_none(), "nothing after the refusal");
}

#[tokio::test]
async fn an_endless_event_of_many_short_lines_ends_with_the_error() {
    let endless = endless_of(b"data: x\n");
    let mut stream = Box::pin(events(endless, 1024));
    let first = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("the reader must stop buffering, not hang");
    assert!(matches!(
        first,
        Some(Err(ClientError::EventTooLarge { .. }))
    ));
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn the_cap_is_per_event_not_cumulative() {
    let out = collect(vec![event_of(100), event_of(100), event_of(100)], 100).await;
    assert_eq!(out.len(), 3);
    assert!(out.iter().all(Result::is_ok));
}

#[tokio::test]
async fn a_run_of_blank_lines_in_one_chunk_is_parsed_in_linear_time() {
    // 256 KiB of nothing but delimiters: 131072 empty blocks in one chunk.
    // Draining and re-searching per block took minutes (cratestack#1379
    // review); linear parsing takes milliseconds even in a debug build.
    for unit in [&b"\n"[..], &b"\r\n"[..]] {
        let body = unit.repeat((256 << 10) / unit.len());
        let started = std::time::Instant::now();
        let out = collect(vec![body, b"data: end\n\n".to_vec()], 1 << 20).await;
        let took = started.elapsed();
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].as_ref().unwrap().data, "end");
        assert!(took.as_secs() < 2, "quadratic parsing: {took:?}");
    }
}
