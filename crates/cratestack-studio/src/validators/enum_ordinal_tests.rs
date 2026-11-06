//! Ordinal enum payload validation (`docs/design/enum-ordinals.md`): a
//! variant's name or one of its ordinals in, anything else out, with the
//! accepted pairs named. A string enum is unchanged.

use super::{ValidationCode, validate_payload};

const SCHEMA: &str = r#"
    enum Status {
      Pending = 1
      Approved = 2

      @@reserved(3)
    }

    enum Channel {
      card
    }

    model Payment {
      id String @id
      status Status
      channel Channel
    }
"#;

fn errors_for(status: serde_json::Value) -> Vec<super::FieldError> {
    let schema = cratestack_parser::parse_schema(SCHEMA).expect("schema parses");
    let model = schema.models.iter().find(|m| m.name == "Payment").unwrap();
    let payload = [
        ("id".to_owned(), serde_json::json!("p1")),
        ("status".to_owned(), status),
        ("channel".to_owned(), serde_json::json!("card")),
    ]
    .into_iter()
    .collect();
    validate_payload(&schema, model, &payload, false)
}

#[test]
fn a_name_or_an_ordinal_is_accepted() {
    for ok in [serde_json::json!("Approved"), serde_json::json!(1)] {
        assert!(errors_for(ok.clone()).is_empty(), "{ok}");
    }
}

#[test]
fn an_unknown_name_a_reserved_number_or_another_type_is_refused() {
    for bad in [
        serde_json::json!("Settled"),
        serde_json::json!(3),
        serde_json::json!(true),
        serde_json::json!(2.5),
    ] {
        let errors = errors_for(bad.clone());
        assert_eq!(errors.len(), 1, "{bad}: {errors:?}");
        assert_eq!(errors[0].code, ValidationCode::TypeMismatch);
        assert!(
            errors[0].message.contains("Pending (1), Approved (2)"),
            "{bad}: {}",
            errors[0].message
        );
    }
}
