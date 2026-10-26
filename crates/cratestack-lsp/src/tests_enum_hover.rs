//! Enum hover: the declaration lists its variants, and an ordinal enum
//! (`Pending = 1`, `docs/design/enum-ordinals.md`) shows each variant's
//! number and the retired ones, from the declaration, a variant, or a
//! field typed by the enum.

use std::str::FromStr;

use tower_lsp_server::ls_types::Uri;

use crate::analyze::analyze_document;
use crate::hover::locate_symbol;
use crate::state::SymbolInfo;

const TEXT: &str = "enum Status {
  Pending = 1
  /// Paid out.
  Approved = 2

  @@reserved(3)
}

enum Role {
  admin
  member
}

model Payment {
  id Int @id
  status Status
  role Role
}
";

fn hover(needle: &str) -> SymbolInfo {
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (schema, diagnostics) = analyze_document(&uri, TEXT);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let schema = schema.expect("schema should parse");
    let offset = TEXT.find(needle).expect("needle should exist");
    locate_symbol(&schema, offset).expect("symbol should resolve")
}

#[test]
fn an_ordinal_enum_lists_its_numbers_and_retired_ones() {
    let symbol = hover("Status {");
    assert_eq!(symbol.kind, "enum");
    assert_eq!(
        symbol.detail,
        "ordinal enum: Pending = 1, Approved = 2; reserved 3"
    );
    // The same from a field typed by it.
    assert_eq!(hover("Status\n  role").detail, symbol.detail);
}

#[test]
fn a_variant_shows_its_ordinal_and_docs() {
    let symbol = hover("Approved");
    assert_eq!(symbol.kind, "enum variant");
    assert_eq!(symbol.detail, "Status.Approved = 2");
    assert_eq!(symbol.docs, vec!["Paid out.".to_owned()]);
}

#[test]
fn a_string_enum_lists_its_names() {
    assert_eq!(hover("Role {").detail, "enum: admin | member");
    assert_eq!(hover("member").detail, "variant of Role");
}
