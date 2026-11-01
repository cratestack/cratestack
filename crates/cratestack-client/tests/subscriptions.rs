//! `include_client_schema!` generates the subscription surface (slice S6):
//! `subscribe` on an `@@subscribe` model's client and a method per
//! subscription procedure, both resolving to a `ClientSubscription`. Against
//! an address nothing listens on, opening one is an `Err` of the call, not a
//! stream item.

use cratestack::client_rust::{ClientConfig, CratestackClient, SubscriptionTransport};

cratestack::include_client_schema!("tests/fixtures/subscriptions.cstack");

#[tokio::test]
async fn subscriptions_are_typed_calls_that_fail_before_a_stream() {
    let config = ClientConfig::new("http://127.0.0.1:9/".parse().unwrap())
        .with_subscription_transport(SubscriptionTransport::Sse);
    let client = cratestack_schema::client::Client::new(CratestackClient::cbor(config));
    let feed: Result<
        cratestack::client_rust::ClientSubscription<
            cratestack::ModelEvent<cratestack_schema::Note>,
        >,
        _,
    > = client.notes().subscribe(&Default::default()).await;
    assert!(feed.is_err());
    let args = cratestack_schema::procedures::lines::Args {
        args: cratestack_schema::Room {
            roomId: "r1".to_owned(),
        },
    };
    let lines: Result<cratestack::client_rust::ClientSubscription<cratestack_schema::Line>, _> =
        client.procedures().lines(&args).await;
    assert!(lines.is_err());
}
