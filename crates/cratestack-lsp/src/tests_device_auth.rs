//! `@device_auth` (cratestack#1303) in the editor: completion offers the
//! attribute with its roles, hover names the role and the
//! `DeviceAuthServer` method, and a marker the parser refuses is a
//! diagnostic on the marker's line.

use std::str::FromStr;

use cratestack_core::schema::DeviceAuthRole;
use tower_lsp_server::ls_types::{CompletionItemKind, Uri};

use crate::analyze::analyze_document;
use crate::completion::completion_items;
use crate::hover::locate_symbol;

const SCHEMA: &str = include_str!("../../cratestack-parser/tests/fixtures/device_auth.cstack");

fn uri() -> Uri {
    Uri::from_str("file:///schema.cstack").expect("uri should parse")
}

#[test]
fn completion_offers_the_attribute_with_every_role_in_the_detail() {
    let items = completion_items(None, None);
    let item = items
        .iter()
        .find(|item| item.label == "@device_auth")
        .expect("completion must offer `@device_auth`");
    assert_eq!(item.kind, Some(CompletionItemKind::KEYWORD));
    let detail = item.detail.as_deref().unwrap_or_default();
    for role in DeviceAuthRole::ALL {
        assert!(detail.contains(role.name()), "{role:?}: {detail}");
    }
}

#[test]
fn hover_names_the_role_and_the_interface_method_of_each_marker() {
    let (schema, diagnostics) = analyze_document(&uri(), SCHEMA);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let schema = schema.expect("valid device auth schema");
    for role in DeviceAuthRole::ALL {
        let marker = format!("@device_auth(role: \"{}\")", role.name());
        let offset = SCHEMA.find(&marker).expect("marker in schema") + 3;
        let symbol = locate_symbol(&schema, offset).expect("hover target");
        assert_eq!(
            (symbol.kind, symbol.name.as_str()),
            ("device auth", role.name())
        );
        assert!(symbol.detail.contains("procedure `"), "{}", symbol.detail);
        if role != DeviceAuthRole::SendEnrolmentCode {
            assert!(
                symbol
                    .detail
                    .contains(&format!("DeviceAuthServer.{}", role.name())),
                "{}",
                symbol.detail
            );
        }
    }
}

#[test]
fn a_refused_marker_is_a_diagnostic_on_its_line() {
    let text = SCHEMA.replace(r#"role: "revoke""#, r#"role: "revokeDevice""#);
    let (_, diagnostics) = analyze_document(&uri(), &text);
    let line = text
        .lines()
        .position(|l| l.contains("revokeDevice\")"))
        .expect("marker line");
    let found = diagnostics
        .iter()
        .find(|d| d.message.contains("has no role `revokeDevice`"))
        .unwrap_or_else(|| panic!("a diagnostic for the role: {diagnostics:?}"));
    assert_eq!(found.range.start.line as usize, line);
}
