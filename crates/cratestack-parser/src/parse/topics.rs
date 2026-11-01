//! `topic RoomMessage(roomId: String): ChatMessage`
//! (`docs/design/subscriptions-websocket.md` §3.4, slice S4): one line, one
//! key, one payload type. A topic takes no attributes: it has no route and
//! no policy of its own.

use std::collections::BTreeMap;

use cratestack_core::{SourceSpan, Topic};

use crate::diagnostics::SchemaError;
use crate::line_helpers::{Line, name_span_in_line};
use crate::parse::procedures::parse_procedure_args;
use crate::parse::types::parse_type_ref;

pub(super) fn parse_topic(line: &Line<'_>, docs: Vec<String>) -> Result<Topic, SchemaError> {
    let error = |message: &str| {
        SchemaError::new(
            message.to_owned(),
            line.start..line.start + line.raw.len(),
            line.number,
        )
    };
    let signature = line
        .trimmed
        .strip_prefix("topic ")
        .ok_or_else(|| error("expected topic declaration"))?;
    let (open, close) = signature
        .find('(')
        .zip(signature.rfind(')'))
        .ok_or_else(|| {
            error("a topic declares its key in parentheses: `topic Name(key: String): Payload`")
        })?;
    let name = signature[..open].trim();
    let key_src = signature[open + 1..close].trim();
    let payload_src = signature[close + 1..]
        .trim()
        .strip_prefix(':')
        .map(str::trim)
        .filter(|payload| !payload.is_empty())
        .ok_or_else(|| {
            error("a topic declares its payload type: `topic Name(key: String): Payload`")
        })?;
    let mut keys = parse_procedure_args(key_src, line, &BTreeMap::new())?;
    if keys.len() != 1 {
        return Err(error(
            "a topic has exactly one key: `topic Name(key: String): Payload`",
        ));
    }
    let payload_offset = line
        .raw
        .rfind(payload_src)
        .ok_or_else(|| error("failed to locate the topic's payload type"))?;
    Ok(Topic {
        docs,
        name: name.to_owned(),
        name_span: name_span_in_line(line, line.trimmed, "topic ")?,
        key: keys.remove(0),
        payload: parse_type_ref(payload_src, line, payload_offset)?,
        span: SourceSpan {
            start: line.start,
            end: line.start + line.raw.len(),
            line: line.number,
        },
    })
}
