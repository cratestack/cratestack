use super::notify_payload;

#[test]
fn a_large_payload_is_sent_by_reference_when_it_can_be() {
    let small = notify_payload::encode("c", "{}", None).unwrap();
    assert!(small.contains("\"p\""));
    let big = "x".repeat(notify_payload::NOTIFY_LIMIT);
    let id = uuid::Uuid::nil();
    let by_ref = notify_payload::encode("c", &big, Some(id)).unwrap();
    assert!(by_ref.contains("\"r\"") && !by_ref.contains("\"p\""));
    assert!(notify_payload::encode("c", &big, None).is_none());
}
