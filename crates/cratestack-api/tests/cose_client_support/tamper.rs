//! What a misbehaving proxy does to the traffic (see `proxy.rs`).

use cratestack::axum::http::StatusCode;

/// What the proxy does to the traffic passing through it.
#[derive(Debug, Clone)]
pub enum Tamper {
    /// Forward faithfully.
    Pass,
    /// Answer with the response recorded for the *previous* request, sealed
    /// for that one: a replay of a genuine, validly signed answer.
    ReplayPrevious,
    /// Forward faithfully but change the status to this one.
    Status(StatusCode),
    /// Answer `200` with this body as plain `application/cbor`: the seal
    /// stripped (or never applied) and the payload forged.
    Plain(Vec<u8>),
    /// Remove this request header before forwarding.
    DropRequestHeader(&'static str),
    /// Answer every request but one for `/redirected` with this redirect
    /// status (`303`, `307`) and `Location: /redirected`, without asking the
    /// server: a hop that would move a sealed request somewhere else.
    Redirect(u16),
    /// Forward faithfully but drop the `Cratestack-Frames` marker an
    /// unsigned subscription stream carries (cratestack#1378).
    StripFrames,
    /// Forward faithfully, marker included, under this `Content-Type`.
    ContentType(&'static str),
    /// Answer with this status and this body as plain `application/cbor`,
    /// without asking the server for anything it would not say itself: an
    /// unsigned refusal a hop made up.
    Answer(StatusCode, Vec<u8>),
}
