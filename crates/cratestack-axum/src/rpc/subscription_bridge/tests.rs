use futures_util::StreamExt;

use super::*;

#[tokio::test]
async fn pushed_items_arrive_in_order_over_the_stream() {
    let (push, rx) = subscription_channel::<u32>();
    push.push(1);
    push.push(2);
    push.push(3);
    drop(push);

    let stream = guarded_receiver_stream(rx, ());
    let items: Vec<u32> = stream.collect().await;
    assert_eq!(items, vec![1, 2, 3]);
}

#[tokio::test]
async fn overflow_closes_the_channel_so_the_stream_ends() {
    // Capacity is `SUBSCRIPTION_BUFFER_CAPACITY` (64) — push well past
    // it without ever reading, so `try_send` is guaranteed to fail at
    // least once.
    let (push, rx) = subscription_channel::<u32>();
    for value in 0..(SUBSCRIPTION_BUFFER_CAPACITY as u32 * 2) {
        push.push(value);
    }
    drop(push);

    let stream = guarded_receiver_stream(rx, ());
    let items: Vec<u32> = stream.collect().await;
    // Some prefix of items got through before the buffer filled; the
    // exact count isn't the point (`try_send` capacity semantics), only
    // that the stream terminates instead of hanging or panicking.
    assert!(!items.is_empty());
    assert!(items.len() <= SUBSCRIPTION_BUFFER_CAPACITY);
}

#[tokio::test]
async fn push_after_overflow_is_a_silent_no_op_not_a_panic() {
    let (push, rx) = subscription_channel::<u32>();
    for value in 0..(SUBSCRIPTION_BUFFER_CAPACITY as u32 * 2) {
        push.push(value);
    }
    // Further pushes on any clone, including a fresh clone, must not
    // panic even though the slot is already closed.
    let push_clone = push.clone();
    push_clone.push(9999);
    drop(push);
    drop(push_clone);

    let stream = guarded_receiver_stream(rx, ());
    let _items: Vec<u32> = stream.collect().await;
}

#[tokio::test]
async fn guard_drops_when_the_stream_is_fully_consumed() {
    struct DropSignal(std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (push, rx) = subscription_channel::<u32>();
    push.push(1);
    drop(push);

    let guard = DropSignal(std::sync::Arc::clone(&dropped));
    let stream = guarded_receiver_stream(rx, guard);
    let items: Vec<u32> = stream.collect().await;
    assert_eq!(items, vec![1]);
    assert!(
        dropped.load(std::sync::atomic::Ordering::SeqCst),
        "guard should have been dropped once the stream was exhausted"
    );
}

#[tokio::test]
async fn guard_drops_when_the_stream_is_cancelled_mid_poll() {
    struct DropSignal(std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // No pushes at all — `rx.recv()` blocks forever, simulating a client
    // that's connected but has received nothing yet.
    let (_push, rx) = subscription_channel::<u32>();
    let guard = DropSignal(std::sync::Arc::clone(&dropped));
    let stream = guarded_receiver_stream(rx, guard);

    // Cancel mid-poll, exactly like an axum response body future being
    // dropped on client disconnect.
    drop(stream);

    assert!(
        dropped.load(std::sync::atomic::Ordering::SeqCst),
        "guard should drop when the stream itself is dropped mid-poll"
    );
}
