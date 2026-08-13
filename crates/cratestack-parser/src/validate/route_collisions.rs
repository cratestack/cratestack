//! Pluralized REST-route-segment collision detection for `model`
//! declarations — split out from `super::snake_case_collisions` (which
//! handles `to_snake_case`-only normalization) because the two
//! normalizations disagree on real inputs: `model Bus` and `model Buse`
//! have distinct `to_snake_case` forms (`bus`/`buse`) but the identical
//! pluralized route segment (`buses`), so a schema can pass the
//! `to_snake_case` check and still collide here.

use cratestack_core::Model;
use cratestack_core::route_naming::model_route_segment;

use crate::diagnostics::{SchemaError, span_error};
use crate::validate::snake_case_collisions::find_collision_by;

/// Reject two `model` declarations whose *pluralized* REST route
/// segments collide — [`cratestack_core::route_naming::model_route_segment`],
/// i.e. `pluralize(to_snake_case(model.name))` — even when their
/// `to_snake_case` forms (what
/// `super::snake_case_collisions::validate_model_name_collisions`
/// checks) differ. `model Bus` and `model Buse` are the smallest real
/// repro: `to_snake_case` gives `bus` and `buse` respectively (distinct,
/// so the snake_case check passes), but `pluralize` gives `buses` for
/// both, since `pluralize` appends `es` to anything already ending in
/// `s` (`bus` -> `buses`) and a bare `s` to anything else (`buse` ->
/// `buses`). Both models would then route to `/buses`: the real Axum
/// server panics at startup registering the second model's routes over
/// the first's (`axum::Router::route` panics on an exact-path/method
/// overlap), and `cratestack-mock-wiremock`'s generated stub silently
/// serves whichever model's mapping WireMock's matcher picks first,
/// dropping the other model's fields with no error at all — see
/// `docs/design/wiremock-stubs.md`'s "Model CRUD statefulness" section.
///
/// Deliberately a schema-wide parser check, not left to
/// `cratestack-mock-wiremock` (or any other single consumer) to guard
/// defensively: the real server hits this exact collision too, so
/// fixing it once here closes the gap for every codegen target, not
/// just the mock.
pub(super) fn validate_model_route_collisions(models: &[Model]) -> Result<(), SchemaError> {
    let entries = models
        .iter()
        .map(|model| (model.name.as_str(), model.name_span));
    if let Some((existing, colliding, span, normalized)) =
        find_collision_by(entries, model_route_segment)
    {
        return Err(span_error(
            format!(
                "model `{colliding}` collides with model `{existing}` — both route to \
                 `/{normalized}` (see `cratestack_core::route_naming::model_route_segment`); \
                 rename one of them",
            ),
            span,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cratestack_core::SourceSpan;

    use super::*;

    fn model(name: &str) -> Model {
        Model {
            docs: Vec::new(),
            name: name.to_owned(),
            name_span: SourceSpan {
                start: 0,
                end: 0,
                line: 1,
            },
            fields: Vec::new(),
            attributes: Vec::new(),
            span: SourceSpan {
                start: 0,
                end: 0,
                line: 1,
            },
        }
    }

    #[test]
    fn bus_and_buse_collide_on_the_pluralized_route_segment() {
        let error = validate_model_route_collisions(&[model("Bus"), model("Buse")])
            .expect_err("`Bus`/`Buse` both route to `/buses`");
        let message = error.to_string();
        assert!(message.contains("Bus") && message.contains("Buse") && message.contains("buses"));
    }

    #[test]
    fn distinct_route_segments_pass() {
        assert!(validate_model_route_collisions(&[model("Bus"), model("Car")]).is_ok());
    }
}
