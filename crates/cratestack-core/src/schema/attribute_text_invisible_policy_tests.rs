//! Variation selectors in policy attributes (module doc, first
//! adjustment): refused wherever they stand, where outside a policy one is
//! allowed after a visible non-ASCII character.

use super::{invisible_character, invisible_character_in_policy, is_policy_attribute};

fn selectors() -> impl Iterator<Item = char> {
    ('\u{FE00}'..='\u{FE0F}').chain('\u{E0100}'..='\u{E01EF}')
}

/// Every policy form, with `{v}` where the role or action text goes.
const POLICIES: [&str; 7] = [
    "@deny(hasRole(\"{v}\"))",
    "@allow(hasRole(\"{v}\"))",
    "@authorize(Doc{v}, read, args.id)",
    "@@deny(\"all\", auth().role == \"{v}\")",
    "@@allow('read', auth().role == '{v}')",
    // Any spelling that reads as a policy, so a misspelled one that is
    // refused for its spelling is not also let through here.
    "@ Deny (hasRole(\"{v}\"))",
    "@no_idempotency @deny(hasRole(\"{v}\"))",
];

/// The measured shapes — a role ending in `é` or in CJK, then a selector —
/// and an emoji, which outside a policy keeps its selector: every one of
/// the 256 selectors is refused in every policy form, at its own offset.
#[test]
fn a_variation_selector_in_a_policy_is_refused_after_any_character() {
    for base in ["banné", "管理者", "\u{2764}", "admin"] {
        for selector in selectors() {
            for policy in POLICIES {
                let text = policy.replace("{v}", &format!("{base}{selector}"));
                let at = text.find(selector).unwrap();
                assert_eq!(invisible_character(&text), Some((at, selector)), "{text:?}");
            }
        }
    }
}

/// Outside a policy attribute the earlier rule stands: after a visible
/// non-ASCII character a selector is allowed, after ASCII it is refused.
#[test]
fn outside_a_policy_a_selector_after_a_visible_character_stays_allowed() {
    for text in [
        "@default(\"\u{2764}\u{FE0F}\")",
        "@default(\"banné\u{FE0F}\")",
        "@default(\"\u{8FBA}\u{E0100}\")",
        "@@server_sql(\"\"\"SELECT '\u{2764}\u{FE0F}' AS allow\"\"\")",
        "@@index([allowed, denied])",
    ] {
        assert!(!is_policy_attribute(text), "{text:?}");
        assert_eq!(invisible_character(text), None, "{text:?}");
    }
    assert_eq!(
        invisible_character("@default(\"n\u{FE0F}\")"),
        Some((11, '\u{FE0F}'))
    );
}

#[test]
fn which_attributes_are_policies() {
    for text in [
        "@allow(true)",
        "@deny(true)",
        "@authorize(Doc, read, args.id)",
        "@@allow(\"read\", true)",
        "@@deny(\"read\", true)",
        "@@DENY(\"read\", true)",
        "@@de\u{200B}ny(\"read\", true)",
        "@default(\"x\")@deny(true)",
    ] {
        assert!(is_policy_attribute(text), "{text:?}");
    }
    for text in [
        "@default(\"@deny\")",
        "@@index([allow])",
        "@allowed",
        "@@audit",
    ] {
        assert!(!is_policy_attribute(text), "{text:?}");
    }
}

/// The caller that already knows the text is a policy gets the same rule
/// without the name being read again.
#[test]
fn the_policy_entry_point_refuses_every_selector() {
    assert_eq!(
        invisible_character_in_policy("\u{2764}\u{FE0F}"),
        Some((3, '\u{FE0F}'))
    );
    assert_eq!(invisible_character_in_policy("\u{2764}"), None);
    assert_eq!(invisible_character("\u{2764}\u{FE0F}"), None);
}
