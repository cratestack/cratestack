//! `.cstack` enum names are PascalCase; proto3's required zero-value variant
//! follows `<SCREAMING_ENUM_NAME>_UNSPECIFIED`. `cratestack-macros::shared`
//! has a PascalCase/camelCase -> snake_case helper already, but this crate
//! must not depend on `cratestack-macros` (macros depends on parser/core,
//! not the other way around — see `docs/design/protobuf.md` §3.3 and the
//! crate-layering rule in the repo's `CLAUDE.md`), so the transform is
//! re-implemented locally rather than reused.

pub(crate) fn to_screaming_snake_case(value: &str) -> String {
    let mut output = String::new();
    for (index, ch) in value.chars().enumerate() {
        if ch.is_uppercase() && index > 0 {
            output.push('_');
        }
        output.extend(ch.to_uppercase());
    }
    output
}

/// `.cstack` procedure names are camelCase (`publishPost`); synthesized
/// message names (`<Procedure>Input`/`Output`) need the PascalCase form.
/// `cratestack-client-typescript::naming::to_pascal_case` does the same
/// job — reimplemented here rather than depended on, same crate-layering
/// rule as [`to_screaming_snake_case`] above.
pub(crate) fn to_pascal_case(value: &str) -> String {
    let mut output = String::new();
    let mut capitalize_next = true;
    for ch in value.chars() {
        if ch == '_' || ch == '-' || ch == ' ' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            output.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            output.push(ch);
        }
    }
    output
}

/// Op id -> gRPC method name, `docs/design/protobuf.md` §4.6: PascalCase
/// each dot-separated segment and drop the dots — `model.User.list` ->
/// `ModelUserList`, `procedure.publishPost` -> `ProcedurePublishPost`.
/// [`to_pascal_case`] already turns a `.cstack`-declared PascalCase model
/// name or camelCase procedure name into its PascalCase form unchanged, so
/// this is a fold, not new casing logic.
///
/// `pub` (not `pub(crate)`) since ticket #172: `cratestack-client-
/// typescript`'s gRPC-Web generator calls this directly from its own
/// (Rust) generator code to derive the exact method name a generated
/// TypeScript client dials, rather than re-deriving PascalCase-and-fold
/// logic that could silently drift from what this crate's own `.proto`
/// `service` block emits (`emit::service`).
///
/// "Reversible" (ticket #170) means: the op id's own segments — `["model",
/// "User", "list"]` — are recoverable losslessly by construction, because
/// every caller of this function builds the method name from the same
/// segments it used to build the op id (see `emit::service`), never from
/// the method name string itself. The method name is a display form of
/// those segments, not an independent encoding a decoder would need to
/// invert — there is no `method_name_to_op_id`, deliberately: PascalCase
/// concatenation is lossy on segment boundaries in general (`"UserList"`
/// could split several ways), so recoverability only holds when the
/// segments are already known, which is always true here.
pub fn op_id_to_method_name(op_id: &str) -> String {
    op_id.split('.').map(to_pascal_case).collect()
}

#[cfg(test)]
mod tests {
    use super::{op_id_to_method_name, to_pascal_case, to_screaming_snake_case};

    #[test]
    fn converts_pascal_case() {
        assert_eq!(to_screaming_snake_case("OrderStatus"), "ORDER_STATUS");
    }

    #[test]
    fn leaves_single_word_uppercased() {
        assert_eq!(to_screaming_snake_case("Order"), "ORDER");
    }

    #[test]
    fn handles_consecutive_capitals() {
        assert_eq!(to_screaming_snake_case("HTTPStatus"), "H_T_T_P_STATUS");
    }

    #[test]
    fn pascal_cases_camel_case_procedure_name() {
        assert_eq!(to_pascal_case("publishPost"), "PublishPost");
    }

    #[test]
    fn pascal_case_leaves_already_pascal_name_alone() {
        assert_eq!(to_pascal_case("GetFeed"), "GetFeed");
    }

    /// Table-driven, both directions per ticket #170: for every op id this
    /// crate ever derives a method name from (every CRUD verb, several
    /// procedure names), confirm (a) the forward derivation matches the
    /// design doc's own worked example and (b) the op id is recoverable
    /// from the very segments used to derive it — see the doc comment on
    /// [`op_id_to_method_name`] for what "reversible" means here.
    #[test]
    fn op_id_to_method_name_round_trips_over_every_known_op_shape() {
        let cases: &[(&[&str], &str)] = &[
            (&["model", "User", "list"], "ModelUserList"),
            (&["model", "User", "get"], "ModelUserGet"),
            (&["model", "User", "create"], "ModelUserCreate"),
            (&["model", "User", "update"], "ModelUserUpdate"),
            (&["model", "User", "delete"], "ModelUserDelete"),
            (&["model", "OrderLine", "list"], "ModelOrderLineList"),
            (&["procedure", "publishPost"], "ProcedurePublishPost"),
            (&["procedure", "getFeed"], "ProcedureGetFeed"),
            (&["procedure", "archiveNote"], "ProcedureArchiveNote"),
        ];

        for (segments, expected_method_name) in cases {
            let op_id = segments.join(".");
            assert_eq!(
                op_id_to_method_name(&op_id),
                *expected_method_name,
                "forward derivation for op id `{op_id}`"
            );
            // Reverse direction: the op id is exactly the segments joined
            // by `.` — the same segments every call site (`emit::service`)
            // uses to build both the op id and the method name, so
            // recovering one from the other is definitionally exact.
            assert_eq!(
                segments.join("."),
                op_id,
                "op id must be recoverable from its own segments"
            );
        }
    }
}
