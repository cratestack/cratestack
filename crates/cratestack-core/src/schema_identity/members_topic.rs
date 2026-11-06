//! A `topic`'s canonical node (slice S4): its name, key and payload type.

use super::canon::CTopic;
use super::members::{args, type_ref};
use crate::schema::Topic;

pub(super) fn topic(t: &Topic) -> CTopic<'_> {
    let Topic {
        docs: _,
        name,
        name_span: _,
        key,
        payload,
        span: _,
    } = t;
    CTopic {
        key: args(std::slice::from_ref(key)).remove(0),
        name,
        payload: type_ref(payload),
    }
}
