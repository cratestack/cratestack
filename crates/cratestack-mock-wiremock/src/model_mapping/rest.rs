//! REST route derivation for a model's five CRUD stubs — mirrors
//! `generate_model_axum_routes`
//! (`crates/cratestack-macros/src/axum/model/routes.rs`): `list`/
//! `create` share `/<plural>`, `get`/`update`/`delete` share
//! `/<plural>/{id}`. `plural` itself must already be
//! `cratestack_core::route_naming::model_route_segment(&model.name)` —
//! computed once by the caller, not re-derived here.

use super::VerbRoute;

/// `{id}` is a WireMock `urlPathPattern` (regex), not a `urlPath` — this
/// generator has no record store to know which ids exist (see
/// `docs/design/wiremock-stubs.md`'s model-CRUD statefulness
/// investigation), so `get`/`update`/`delete` match *any* id and
/// return the same synthesized record every time.
pub(crate) fn rest_routes(base: &str, plural: &str) -> [VerbRoute; 5] {
    let list_path = format!("{base}/{plural}");
    let detail_pattern = format!("^{}/{}/[^/]+$", regex_escape(base), regex_escape(plural));

    [
        VerbRoute {
            verb: "list",
            method: "GET",
            url: list_path.clone(),
            is_pattern: false,
            status: 200,
        },
        VerbRoute {
            verb: "get",
            method: "GET",
            url: detail_pattern.clone(),
            is_pattern: true,
            status: 200,
        },
        VerbRoute {
            verb: "create",
            method: "POST",
            url: list_path,
            is_pattern: false,
            // `crates/cratestack-macros/src/axum/model/handlers_crud.rs`'s
            // `build_create_handler` hardcodes `StatusCode::CREATED`,
            // unlike every procedure's literal `StatusCode::OK` — the one
            // place a model CRUD stub's status differs from the v1
            // procedure-only baseline.
            status: 201,
        },
        VerbRoute {
            verb: "update",
            method: "PATCH",
            url: detail_pattern.clone(),
            is_pattern: true,
            status: 200,
        },
        VerbRoute {
            verb: "delete",
            method: "DELETE",
            url: detail_pattern,
            is_pattern: true,
            status: 200,
        },
    ]
}

/// Escapes Java-regex metacharacters so a `--base-path` (or, in
/// principle, a model route segment) containing one is matched
/// literally in a `urlPathPattern`, not interpreted as regex syntax.
/// `model_route_segment`'s own output is always
/// `[a-zA-Z0-9_]+`-shaped (see `cratestack_core::route_naming`'s own
/// doc comments), so this only ever has real work to do on `base`.
fn regex_escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(
            ch,
            '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\'
        ) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::regex_escape;

    #[test]
    fn escapes_regex_metacharacters() {
        assert_eq!(regex_escape("/api.v2"), "/api\\.v2");
        assert_eq!(regex_escape("/api"), "/api");
    }
}
