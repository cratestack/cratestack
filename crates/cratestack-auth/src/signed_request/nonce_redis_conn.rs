//! The Redis nonce store's one shared connection, which can be thrown away
//! and rebuilt (cratestack#1070).
//!
//! `redis::aio::ConnectionManager` reconnects by itself only when its stored
//! connect future failed with an I/O error. A reconnect that fails any other
//! way (`AuthenticationFailed` after a password change, a `SELECT` or
//! `HELLO` error) stays stored, and every later command returns it until
//! the process restarts; a response timeout never reconnects at all. So the
//! store does not trust a manager that has failed: [`SharedConnection::reset`]
//! drops it and the next claim builds a fresh one.

use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use redis::{Client as RedisClient, ErrorKind, ServerErrorKind};
use tokio::sync::OnceCell;

/// The driver's own defaults, made explicit: one connect attempt gets 1 s
/// and one reply 500 ms. Deliberately not the 2 s used by `cratestack-redis`:
/// this path is on every signed request.
const CONNECT_TIMEOUT: StdDuration = StdDuration::from_secs(1);
const RESPONSE_TIMEOUT: StdDuration = StdDuration::from_millis(500);

/// One connect attempt and its outcome, shared by every claim that arrived
/// while it ran. The error is text: it only ever becomes
/// `NonceStoreUnavailable`.
pub(super) type Attempt = Arc<OnceCell<Result<ConnectionManager, String>>>;

pub(super) struct SharedConnection {
    client: RedisClient,
    /// Locked only to clone or swap the `Arc`, never across an `.await`.
    current: Mutex<Option<Attempt>>,
}

impl SharedConnection {
    pub(super) fn new(client: RedisClient) -> Self {
        Self {
            client,
            current: Mutex::new(None),
        }
    }

    /// The current attempt, started empty if there is none. Claims that
    /// find an attempt already running share its single connect (at most
    /// [`CONNECT_TIMEOUT`]); they do not queue one attempt each.
    pub(super) fn attempt(&self) -> Attempt {
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        current.get_or_insert_with(Attempt::default).clone()
    }

    /// The manager of `attempt`, connecting if this is the first claim to
    /// get here. On failure the attempt is discarded so the next claim
    /// makes a fresh one; the failed claim is not retried.
    pub(super) async fn manager(&self, attempt: &Attempt) -> Result<ConnectionManager, String> {
        let outcome = attempt
            .get_or_init(|| async {
                ConnectionManager::new_with_config(self.client.clone(), manager_config())
                    .await
                    .map_err(|error| error.to_string())
            })
            .await;
        match outcome {
            Ok(manager) => Ok(manager.clone()),
            Err(error) => {
                self.reset(attempt);
                Err(error.clone())
            }
        }
    }

    /// Drops `attempt` if it is still the current one; a claim holding an
    /// older attempt cannot discard a newer, healthy connection.
    pub(super) fn reset(&self, attempt: &Attempt) {
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if current.as_ref().is_some_and(|c| Arc::ptr_eq(c, attempt)) {
            *current = None;
        }
    }
}

/// No reconnect backoff: the driver's default of six retries makes a claim
/// wait out roughly nine seconds of an outage, on the request path. The
/// store resets and reconnects itself on the next claim instead.
fn manager_config() -> ConnectionManagerConfig {
    ConnectionManagerConfig::new()
        .set_connection_timeout(Some(CONNECT_TIMEOUT))
        .set_response_timeout(Some(RESPONSE_TIMEOUT))
        .set_number_of_retries(0)
}

/// True when the server answered our command with an error reply. The
/// connection is healthy and stays. Everything else (I/O, timeout, a
/// dropped connection, a parse or auth error) means the connection can no
/// longer be trusted. `READONLY` is a reply, but it means the socket
/// points at a demoted primary, so it also reconnects.
pub(super) fn is_plain_server_reply(error: &redis::RedisError) -> bool {
    match error.kind() {
        ErrorKind::Server(ServerErrorKind::ReadOnly) => false,
        ErrorKind::Server(_) | ErrorKind::Extension => true,
        _ => false,
    }
}

/// True when the command can not have run: the connection was already gone
/// or broke on the write (broken pipe, reset, closed). A timeout is not
/// that (the command is queued on a socket that may yet execute it), and
/// neither is a refused connect (nothing was sent).
pub(super) fn dropped_before_running(error: &redis::RedisError) -> bool {
    error.is_connection_dropped() && !error.is_timeout() && !error.is_connection_refusal()
}
