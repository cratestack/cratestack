//! Stateful REST model CRUD stub generation — the v2 default for
//! `transport rest` schemas (see `docs/design/wiremock-stubs.md`'s
//! "Model CRUD statefulness" section for the investigation this
//! implements the conclusion of). Unlike the static v1 baseline
//! (`crates/cratestack-mock-wiremock/src/model_record.rs`, still used
//! for `transport rpc` — see that module's doc comment for why), these
//! stubs need `wiremock-state-extension` loaded into the WireMock
//! instance they're served from; a plain `docker run wiremock/wiremock`
//! is not enough. See `crates/cratestack-mock-wiremock/README.md` for
//! what running them costs.

mod body;
mod fields;
mod fragments;
mod if_match;
mod listeners;
mod mapping;
mod version_gate;

use std::collections::BTreeSet;

use cratestack_core::{Model, Schema};
use serde_json::Value;

use crate::config::WireMockGeneratorConfig;
use crate::error::WireMockGeneratorError;
use crate::model_attrs::{is_paged_model, is_primary_key};
use fields::build_field_plan;
use fragments::{read_state, version_bump};
use mapping::{envelope, envelope_with_matcher, regex_escape, with_etag_header};
use version_gate::gated_mappings;

/// Builds the stateful `(verb, mapping)` pairs for `model` under
/// `transport rest`: `["list", "get", "create", "update", "delete"]`
/// for a plain model, or — for an `@version` model — `"update"`/
/// `"delete"` each fanned out into five `If-Match`-gated stubs by
/// [`version_gate::gated_mappings`] (`"update"`, `"update-if-match-
/// required"`, `"update-if-match-wildcard"`, `"update-if-match-
/// malformed"`, `"update-if-match-stale"`, and the `delete` equivalents)
/// alongside an unchanged `"list"`/`"create"`, and a `"get"` whose
/// response now also carries an `ETag`.
pub(crate) fn build_stateful_rest_mappings(
    schema: &Schema,
    config: &WireMockGeneratorConfig,
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> Result<Vec<(String, Value)>, WireMockGeneratorError> {
    let pk_field = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .ok_or_else(|| WireMockGeneratorError::ModelMissingPrimaryKey {
            model: model.name.clone(),
        })?;
    let plan = build_field_plan(schema, model, model_names, pk_field)?;
    let paged = is_paged_model(model);

    let base = config.base_path.trim_end_matches('/');
    let plural = cratestack_core::route_naming::model_route_segment(&model.name);
    let list_path = format!("{base}/{plural}");
    let detail_pattern = format!("^{}/{}/[^/]+$", regex_escape(base), regex_escape(&plural));

    // The shared collection's state-store context is just the plural
    // name. Two distinct models routing to the same plural (e.g. `Bus`
    // and `Buse`, both `/api/buses`) would collide their contexts and
    // silently share one state pool — but `cratestack-parser`'s
    // `validate_model_route_collisions` rejects that schema-wide at
    // parse time (cratestack#588's follow-up), before a `Schema` value
    // can ever reach this generator, so it's a parser invariant this
    // code relies on rather than something this code re-checks. A
    // per-record context, by contrast, must be unique across every
    // record of every model: `request.path` (e.g. `/api/posts/42`) is
    // already exactly that, for free, on every REST detail request — no
    // string-concatenation Handlebars helper needed (there isn't one in
    // this templating stack; confirmed by hand). `create` doesn't have
    // an inbound `request.path` for the *new* record yet, so it builds
    // the identical string from the known detail-route prefix plus the
    // id it just generated.
    let record_context_new = format!(
        "{list_path}/{{{{jsonPath response.body '$.{}'}}}}",
        plan.pk_name
    );
    let has_context_matcher = "{{request.path}}";

    let list_mapping = envelope(
        "GET",
        &list_path,
        200,
        &body::list_body(&plan, &plural, paged),
        None,
        &model.name,
        "list",
    );
    let create_mapping = envelope(
        "POST",
        &list_path,
        201,
        &body::create_body(&plan),
        Some(listeners::create_listeners(
            &plan,
            &plural,
            &record_context_new,
        )),
        &model.name,
        "create",
    );
    let mut get_mapping = envelope_with_matcher(
        "GET",
        &detail_pattern,
        has_context_matcher,
        200,
        &body::read_body(&plan, "request.path"),
        None,
        &model.name,
        "get",
    );
    let update_listeners = Some(listeners::update_listeners(&plan, "{{request.path}}"));
    let delete_listeners = Some(listeners::delete_listeners(
        &plan,
        &plural,
        "{{request.path}}",
    ));

    let mut mappings = vec![
        ("list".to_owned(), list_mapping),
        ("create".to_owned(), create_mapping),
    ];

    match &plan.version_name {
        None => {
            let update_mapping = envelope_with_matcher(
                "PATCH",
                &detail_pattern,
                has_context_matcher,
                200,
                &body::update_body(&plan, "request.path"),
                update_listeners,
                &model.name,
                "update",
            );
            let delete_mapping = envelope_with_matcher(
                "DELETE",
                &detail_pattern,
                has_context_matcher,
                200,
                &body::read_body(&plan, "request.path"),
                delete_listeners,
                &model.name,
                "delete",
            );
            mappings.push(("update".to_owned(), update_mapping));
            mappings.push(("delete".to_owned(), delete_mapping));
        }
        Some(version_name) => {
            get_mapping = with_etag_header(get_mapping, &read_state(version_name, "request.path"));
            mappings.extend(gated_mappings(
                "PATCH",
                &detail_pattern,
                &plan,
                200,
                &body::update_body(&plan, "request.path"),
                update_listeners,
                Some(&version_bump(version_name, "request.path")),
                &model.name,
                "update",
            ));
            mappings.extend(gated_mappings(
                "DELETE",
                &detail_pattern,
                &plan,
                200,
                &body::read_body(&plan, "request.path"),
                delete_listeners,
                None,
                &model.name,
                "delete",
            ));
        }
    }
    // Insertion order doesn't matter beyond this point — `generator.rs`
    // sorts the final file list by name for deterministic output.
    mappings.push(("get".to_owned(), get_mapping));

    Ok(mappings)
}
