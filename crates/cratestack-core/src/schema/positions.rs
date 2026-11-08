//! Source positions are the one part of a [`Schema`] that differs between a
//! schema split across files and the single file it is equivalent to.

use serde_json::{Map, Value};

use super::Schema;

/// `schema` with every [`SourceSpan`](super::SourceSpan) zeroed.
///
/// Two IRs are *identical* (docs/design/multi-file-schemas.md section 8) when
/// they are equal after this: positions differ by definition between a split
/// schema and its concatenated equivalent, and nothing else is excluded.
/// Declaration order, `///` docs and attribute text all still count.
///
/// Implemented over the serialized form: every JSON object whose keys are
/// exactly `start`, `end` and `line` has all three set to `0`. `SourceSpan`
/// is the only IR type with that shape, which a test in `cratestack-parser`
/// pins against a rich schema.
pub fn without_source_positions(schema: &Schema) -> Schema {
    let mut value = serde_json::to_value(schema).expect("a Schema always serializes to JSON");
    zero_spans(&mut value);
    serde_json::from_value(value).expect("a Schema with zeroed spans deserializes again")
}

fn is_span(object: &Map<String, Value>) -> bool {
    object.len() == 3
        && ["start", "end", "line"]
            .iter()
            .all(|key| object.contains_key(*key))
}

fn zero_spans(value: &mut Value) {
    match value {
        Value::Object(object) if is_span(object) => {
            for field in object.values_mut() {
                *field = Value::from(0);
            }
        }
        Value::Object(object) => object.values_mut().for_each(zero_spans),
        Value::Array(items) => items.iter_mut().for_each(zero_spans),
        _ => {}
    }
}

#[cfg(test)]
#[path = "positions_tests.rs"]
mod positions_tests;
