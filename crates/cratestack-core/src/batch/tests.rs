use super::*;

#[test]
fn find_duplicate_position_returns_first_collision() {
    assert_eq!(find_duplicate_position::<i32>([]), None);
    assert_eq!(find_duplicate_position([1, 2, 3]), None);
    // First occurrence index, then duplicate's index.
    assert_eq!(find_duplicate_position([1, 2, 1]), Some((0, 2)));
    assert_eq!(find_duplicate_position([5, 5]), Some((0, 1)));
    // Multiple duplicates — report the first pair encountered.
    assert_eq!(find_duplicate_position([1, 2, 3, 2, 1]), Some((1, 3)));
}

#[test]
fn batch_response_summary_counts_ok_and_err() {
    let response: BatchResponse<i64> = BatchResponse::from_results(vec![
        Ok(10),
        Err(CoolError::NotFound("missing".to_owned())),
        Ok(20),
        Err(CoolError::Forbidden("nope".to_owned())),
    ]);
    assert_eq!(response.summary.total, 4);
    assert_eq!(response.summary.ok, 2);
    assert_eq!(response.summary.err, 2);
    // Index preservation is the whole contract.
    assert_eq!(response.results[0].index, 0);
    assert_eq!(response.results[3].index, 3);
    // Error projection rides CoolError::code().
    match &response.results[1].status {
        BatchItemStatus::Error { error } => assert_eq!(error.code, "NOT_FOUND"),
        BatchItemStatus::Ok { .. } => panic!("expected per-item error"),
    }
}

#[test]
fn batch_item_status_wire_shape_is_tagged_lowercase() {
    // Lock the JSON shape so future serde edits can't silently change
    // it — clients depend on `status: "ok" | "error"` discriminants.
    let ok = BatchItemResult {
        index: 0,
        status: BatchItemStatus::Ok { value: 42i64 },
    };
    let ok_json = serde_json::to_string(&ok).unwrap();
    assert!(ok_json.contains("\"status\":\"ok\""), "got: {ok_json}");
    assert!(ok_json.contains("\"value\":42"), "got: {ok_json}");

    let err = BatchItemResult::<i64> {
        index: 7,
        status: BatchItemStatus::Error {
            error: BatchItemError {
                code: "VALIDATION_ERROR".to_owned(),
                message: "bad input".to_owned(),
            },
        },
    };
    let err_json = serde_json::to_string(&err).unwrap();
    assert!(err_json.contains("\"status\":\"error\""), "got: {err_json}");
    assert!(err_json.contains("\"index\":7"), "got: {err_json}");
    assert!(
        err_json.contains("\"code\":\"VALIDATION_ERROR\""),
        "got: {err_json}",
    );
}
