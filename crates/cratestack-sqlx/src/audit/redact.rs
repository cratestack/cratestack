//! PII/sensitive-column redaction + JSON snapshot helpers used when
//! building audit events. Redaction substitutes a canned marker
//! string; banks need the audit log to record THAT a field changed
//! without retaining the actual value (PAN, SSN, address).

/// Replace values of PII/sensitive columns in a JSON snapshot with a
/// fixed marker. Banks need the audit log to record THAT a field
/// changed without retaining the actual value; the marker lets a
/// human reviewer see the column shifted while keeping the data out
/// of long-term logs.
pub fn redact_snapshot(
    snapshot: &mut serde_json::Value,
    pii_columns: &[&str],
    sensitive_columns: &[&str],
) {
    let Some(map) = snapshot.as_object_mut() else {
        return;
    };
    for col in pii_columns {
        if let Some(slot) = map.get_mut(*col) {
            *slot = serde_json::Value::String("[redacted-pii]".to_owned());
        }
        let camel = snake_to_camel(col);
        if camel != *col {
            if let Some(slot) = map.get_mut(&camel) {
                *slot = serde_json::Value::String("[redacted-pii]".to_owned());
            }
        }
    }
    for col in sensitive_columns {
        if let Some(slot) = map.get_mut(*col) {
            *slot = serde_json::Value::String("[redacted-sensitive]".to_owned());
        }
        let camel = snake_to_camel(col);
        if camel != *col {
            if let Some(slot) = map.get_mut(&camel) {
                *slot = serde_json::Value::String("[redacted-sensitive]".to_owned());
            }
        }
    }
}

/// Convert a model into the JSON snapshot used by the audit log.
/// Returns `None` if the model isn't serializable; that should never
/// happen for generated models which derive `Serialize`.
pub fn snapshot_model<T>(model: &T) -> Option<serde_json::Value>
where
    T: serde::Serialize,
{
    serde_json::to_value(model).ok()
}

/// Extract the primary-key field from a serialized model snapshot.
/// Used to stamp audit events with a stable identifier even when the
/// schema doesn't surface the PK column verbatim in the response.
pub fn primary_key_from_snapshot(
    snapshot: &serde_json::Value,
    primary_key_column: &str,
) -> serde_json::Value {
    if let Some(map) = snapshot.as_object() {
        if let Some(value) = map.get(primary_key_column) {
            return value.clone();
        }
        // Try snake/camel transposition — the SQL column name might
        // differ from the JSON key emitted by the serializer.
        let camel = snake_to_camel(primary_key_column);
        if let Some(value) = map.get(&camel) {
            return value.clone();
        }
    }
    serde_json::Value::Null
}

fn snake_to_camel(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut upper = false;
    for ch in input.chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_primary_key_by_snake_case_column() {
        let snapshot = json!({ "user_id": 42, "balance": "10.00" });
        let pk = primary_key_from_snapshot(&snapshot, "user_id");
        assert_eq!(pk, json!(42));
    }

    #[test]
    fn extracts_primary_key_via_camel_case_fallback() {
        let snapshot = json!({ "userId": 42, "balance": "10.00" });
        let pk = primary_key_from_snapshot(&snapshot, "user_id");
        assert_eq!(pk, json!(42));
    }

    #[test]
    fn returns_null_when_primary_key_absent() {
        let snapshot = json!({ "balance": "10.00" });
        let pk = primary_key_from_snapshot(&snapshot, "user_id");
        assert_eq!(pk, serde_json::Value::Null);
    }

    #[test]
    fn snapshot_round_trip_preserves_strings_and_numbers() {
        let snap =
            snapshot_model(&json!({ "amount": "12.34", "currency": "USD" })).expect("serializable");
        assert_eq!(snap["amount"], json!("12.34"));
        assert_eq!(snap["currency"], json!("USD"));
    }

    #[test]
    fn redacts_pii_columns_with_canned_marker() {
        let mut snap = json!({
            "id": 1,
            "email": "alice@example.com",
            "balance": "10.00",
        });
        redact_snapshot(&mut snap, &["email"], &[]);
        assert_eq!(snap["email"], json!("[redacted-pii]"));
        assert_eq!(snap["balance"], json!("10.00"));
    }

    #[test]
    fn redacts_sensitive_columns_with_distinct_marker() {
        let mut snap = json!({
            "id": 1,
            "risk_score": 87,
        });
        redact_snapshot(&mut snap, &[], &["risk_score"]);
        assert_eq!(snap["risk_score"], json!("[redacted-sensitive]"));
    }

    #[test]
    fn redaction_handles_camel_case_keys() {
        let mut snap = json!({
            "id": 1,
            "primaryEmail": "x@y.com",
        });
        redact_snapshot(&mut snap, &["primary_email"], &[]);
        assert_eq!(snap["primaryEmail"], json!("[redacted-pii]"));
    }

    #[test]
    fn redaction_is_noop_for_absent_columns() {
        let mut snap = json!({ "id": 1 });
        redact_snapshot(&mut snap, &["email"], &["risk_score"]);
        assert_eq!(snap, json!({ "id": 1 }));
    }
}
