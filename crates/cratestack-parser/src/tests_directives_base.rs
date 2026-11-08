//! `parse_file_at` shifts every span by the file's base offset
//! (docs/design/multi-file-schemas.md section 9).

use serde_json::Value;

use crate::parse::parse_file_at;

const RICH_INLINE: &str = r#"extension pgvector {
}
mixin Stamped {
  createdAt DateTime
}
/// A status.
enum Status {
  Open
  Closed
}
type Money {
  amount Decimal
}
model Note {
  @use(Stamped)
  id Int @id
  status Status
  @@allow("read", true)
}
topic Updates(room: String): Money
"#;

/// Every `{start,end,line}` object, with the key it sits under.
fn spans<'a>(value: &'a Value, key: &str, out: &mut Vec<(String, &'a Value)>) {
    match value {
        Value::Object(map) => {
            if map.len() == 3
                && ["start", "end", "line"]
                    .iter()
                    .all(|k| map.contains_key(*k))
            {
                out.push((key.to_owned(), value));
            }
            for (k, v) in map {
                spans(v, k, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| spans(v, key, out)),
        _ => {}
    }
}

fn assert_shifted(base_zero: &Value, shifted: &Value, shift: u64) {
    match (base_zero, shifted) {
        (Value::Object(a), Value::Object(b)) => {
            let is_span =
                a.len() == 3 && ["start", "end", "line"].iter().all(|k| a.contains_key(*k));
            if is_span {
                assert_eq!(
                    b["start"].as_u64().unwrap(),
                    a["start"].as_u64().unwrap() + shift
                );
                assert_eq!(
                    b["end"].as_u64().unwrap(),
                    a["end"].as_u64().unwrap() + shift
                );
                assert_eq!(b["line"], a["line"]);
            } else {
                assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
                for (k, v) in a {
                    assert_shifted(v, &b[k], shift);
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (x, y) in a.iter().zip(b) {
                assert_shifted(x, y, shift);
            }
        }
        (a, b) => assert_eq!(a, b),
    }
}

#[test]
fn parse_at_base_shifts_every_span_and_keeps_lines() {
    let sources = [
        include_str!("../../../examples/react-vite-swr/schema.cstack"),
        include_str!("../../../examples/declarative-query-verification/schema.cstack"),
        RICH_INLINE,
    ];
    for source in sources {
        let at_zero = serde_json::to_value(parse_file_at(source, 0).unwrap().schema).unwrap();
        let at_base = serde_json::to_value(parse_file_at(source, 1000).unwrap().schema).unwrap();
        let mut found = Vec::new();
        spans(&at_zero, "schema", &mut found);
        assert!(!found.is_empty());
        assert_shifted(&at_zero, &at_base, 1000);
    }
}

#[test]
fn source_span_is_the_only_ir_shape_with_start_end_line_keys() {
    let value = serde_json::to_value(parse_file_at(RICH_INLINE, 0).unwrap().schema).unwrap();
    let mut found = Vec::new();
    spans(&value, "schema", &mut found);
    assert!(found.len() > 10);
    for (key, _) in found {
        assert!(
            key.contains("span"),
            "a {{start,end,line}} object sits at `{key}`"
        );
    }
}
