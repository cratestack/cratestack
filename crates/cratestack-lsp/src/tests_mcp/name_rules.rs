//! The two `name` rules beyond a DNS label's (cratestack#1040, 2026-09-25)
//! reach the editor the way the DNS-label ones do (the parent module's
//! `the_mcp_name_is_hinted_and_checked_as_a_dns_label`): IDNA's reserved
//! `??--` form and an all-digit name are each one diagnostic on the entry,
//! naming its rule, and the completion popup states both.

use super::{SCHEMA, uri};
use crate::analyze::analyze_document;
use crate::completion::completion_items;
use crate::text::range_from_offsets;

#[test]
fn the_mcp_name_reserved_form_and_all_digit_names_are_diagnostics() {
    let items = completion_items(None);
    let name = items.iter().find(|item| item.label == "name = \"...\"");
    let detail = name.and_then(|item| item.detail.as_deref()).unwrap_or("");
    assert!(detail.contains("IDNA's reserved form"), "{detail}");
    assert!(detail.contains("at least one letter"), "{detail}");

    for (value, rule) in [
        ("xn--blog", "the form IDNA reserves"),
        ("127", "must contain at least one letter"),
    ] {
        let text = SCHEMA.replace("name = \"blog\"", &format!("name = \"{value}\""));
        let (schema, diagnostics) = analyze_document(&uri(), &text);
        assert!(schema.is_none(), "{value:?}");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains(rule), "{diagnostics:?}");
        let start = text.find("name = ").expect("entry");
        let end = start + format!("name = \"{value}\"").len();
        assert_eq!(diagnostics[0].range, range_from_offsets(&text, start, end));
    }
}
