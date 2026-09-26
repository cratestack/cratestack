//! Shared by `tests/server_only_outbound.rs` (REST and RPC) and
//! `tests/server_only_outbound_mcp.rs` (MCP tools): the leak check, the
//! wire shape every transport must produce, and one procedure registry and
//! resolver set, emitted into each transport's `include_server_schema!`
//! module by [`so_out_impls!`]. In a directory so cargo does not build it
//! as a test binary of its own.

use serde_json::{Value as Json, json};

/// Held only in `SoOutWidget.secret` (`@server_only`).
pub const WIDGET_SECRET: &str = "HUNTER2";
/// Held only in `SoOutWidget.recovery` (`@server_only`, optional).
pub const WIDGET_RECOVERY: &str = "RECOVER-9";
/// Held only in `SoOutPart.token` (`@server_only`).
pub const PART_TOKEN: &str = "PART-TOKEN-7";

/// The fixture's `@server_only` field names.
const SERVER_ONLY_KEYS: [&str; 3] = ["secret", "recovery", "token"];

/// `hint`'s resolver: a value derived from `secret`. Sending it is the
/// schema author's choice; sending `secret` itself is the leak.
pub fn hint_for(secret: &str) -> String {
    format!("{} characters", secret.chars().count())
}

/// The procedure argument every call sends.
pub const LABEL: &str = "alpha";

/// A widget as every transport must send it: its stored fields but
/// `secret`, and both computed fields resolved. The procedures return
/// `secret: WIDGET_SECRET` on every widget, so equality with this is also
/// the proof that the computed path ran.
pub fn expected_widget(id: i64) -> Json {
    json!({
        "id": id,
        "label": LABEL,
        "shout": LABEL.to_uppercase(),
        "hint": hint_for(WIDGET_SECRET),
    })
}

/// Each procedure, the JSON pointer of the part of its output that is
/// compared exactly, and what that part must be. The page envelope
/// (`totalCount`, `pageInfo`) is not the model's shape, so only `items` is
/// compared; the whole body is still leak-checked.
pub fn procedure_cases() -> Vec<(&'static str, &'static str, Json)> {
    vec![
        ("soOutWidget", "", expected_widget(1)),
        ("soOutMaybeWidget", "", expected_widget(1)),
        (
            "soOutWidgets",
            "",
            json!([expected_widget(1), expected_widget(2)]),
        ),
        ("soOutWidgetPage", "/items", json!([expected_widget(1)])),
        (
            "soOutEnvelope",
            "",
            json!({
                "one": expected_widget(1),
                "maybe": expected_widget(2),
                "many": [expected_widget(3)],
                "note": "n",
            }),
        ),
    ]
}

/// Fails if `body` carries a `@server_only` value anywhere in its bytes,
/// whatever the codec, or (when it is JSON) names a `@server_only` key at
/// any depth.
pub fn assert_no_server_only(context: &str, body: &[u8]) {
    let text = String::from_utf8_lossy(body);
    for value in [WIDGET_SECRET, WIDGET_RECOVERY, PART_TOKEN] {
        assert!(
            !text.contains(value),
            "{context}: a @server_only value reached the response: {text}"
        );
    }
    if let Ok(json) = serde_json::from_slice::<Json>(body) {
        assert_no_server_only_key(context, &json);
    }
}

fn assert_no_server_only_key(context: &str, value: &Json) {
    match value {
        Json::Object(object) => {
            for key in SERVER_ONLY_KEYS {
                assert!(
                    !object.contains_key(key),
                    "{context}: @server_only key `{key}` reached the response: {value}"
                );
            }
            for nested in object.values() {
                assert_no_server_only_key(context, nested);
            }
        }
        Json::Array(items) => {
            for item in items {
                assert_no_server_only_key(context, item);
            }
        }
        _ => {}
    }
}

/// `Procedures` (every fixture procedure, each returning widgets whose
/// `secret` is set, as a row loaded from the database would have it) and
/// `Resolvers`, for the `cratestack_schema` in scope at the call site.
macro_rules! so_out_impls {
    () => {
        #[derive(Clone)]
        pub(crate) struct Procedures;

        #[derive(Clone)]
        pub(crate) struct Resolvers;

        fn widget(id: i64, label: &str) -> cratestack_schema::SoOutWidget {
            cratestack_schema::SoOutWidget {
                id,
                label: label.to_owned(),
                secret: crate::server_only_outbound_support::WIDGET_SECRET.to_owned(),
                recovery: Some(crate::server_only_outbound_support::WIDGET_RECOVERY.to_owned()),
            }
        }

        impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
            async fn so_out_widget(
                &self,
                _db: &cratestack_schema::Cratestack,
                _ctx: &cratestack::CratestackContext,
                args: cratestack_schema::procedures::so_out_widget::Args,
                _authorized: cratestack_schema::procedures::so_out_widget::Authorized,
            ) -> Result<cratestack_schema::SoOutWidget, cratestack::CratestackError> {
                Ok(widget(1, &args.label))
            }

            async fn so_out_maybe_widget(
                &self,
                _db: &cratestack_schema::Cratestack,
                _ctx: &cratestack::CratestackContext,
                args: cratestack_schema::procedures::so_out_maybe_widget::Args,
                _authorized: cratestack_schema::procedures::so_out_maybe_widget::Authorized,
            ) -> Result<Option<cratestack_schema::SoOutWidget>, cratestack::CratestackError>
            {
                Ok(Some(widget(1, &args.label)))
            }

            async fn so_out_widgets(
                &self,
                _db: &cratestack_schema::Cratestack,
                _ctx: &cratestack::CratestackContext,
                args: cratestack_schema::procedures::so_out_widgets::Args,
                _authorized: cratestack_schema::procedures::so_out_widgets::Authorized,
            ) -> Result<Vec<cratestack_schema::SoOutWidget>, cratestack::CratestackError> {
                Ok(vec![widget(1, &args.label), widget(2, &args.label)])
            }

            async fn so_out_widget_page(
                &self,
                _db: &cratestack_schema::Cratestack,
                _ctx: &cratestack::CratestackContext,
                args: cratestack_schema::procedures::so_out_widget_page::Args,
                _authorized: cratestack_schema::procedures::so_out_widget_page::Authorized,
            ) -> Result<cratestack::Page<cratestack_schema::SoOutWidget>, cratestack::CratestackError>
            {
                Ok(cratestack::Page::new(
                    vec![widget(1, &args.label)],
                    cratestack::PageInfo {
                        limit: None,
                        offset: None,
                        has_next_page: false,
                        has_previous_page: false,
                    },
                ))
            }

            async fn so_out_envelope(
                &self,
                _db: &cratestack_schema::Cratestack,
                _ctx: &cratestack::CratestackContext,
                args: cratestack_schema::procedures::so_out_envelope::Args,
                _authorized: cratestack_schema::procedures::so_out_envelope::Authorized,
            ) -> Result<cratestack_schema::SoOutEnvelope, cratestack::CratestackError> {
                Ok(cratestack_schema::SoOutEnvelope {
                    one: widget(1, &args.label),
                    maybe: Some(widget(2, &args.label)),
                    many: vec![widget(3, &args.label)],
                    note: "n".to_owned(),
                })
            }
        }

        impl cratestack_schema::ComputedFieldResolver for Resolvers {
            async fn resolve_so_out_widget_shout(
                &self,
                _db: &cratestack_schema::Cratestack,
                source: &cratestack_schema::SoOutWidget,
                _ctx: &cratestack::CratestackContext,
            ) -> Result<String, cratestack::CratestackError> {
                Ok(source.label.to_uppercase())
            }

            async fn resolve_so_out_widget_hint(
                &self,
                _db: &cratestack_schema::Cratestack,
                source: &cratestack_schema::SoOutWidget,
                _ctx: &cratestack::CratestackContext,
            ) -> Result<String, cratestack::CratestackError> {
                Ok(crate::server_only_outbound_support::hint_for(
                    &source.secret,
                ))
            }

            async fn resolve_so_out_part_loud(
                &self,
                _db: &cratestack_schema::Cratestack,
                source: &cratestack_schema::SoOutPart,
                _ctx: &cratestack::CratestackContext,
            ) -> Result<String, cratestack::CratestackError> {
                Ok(source.name.to_uppercase())
            }
        }
    };
}
