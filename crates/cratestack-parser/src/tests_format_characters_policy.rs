//! GHSA-69g4-xvcm-vm2j, calls decided under the maintainer's standing secure-default rule,
//! on top of maintainer decision 3
//! (`cratestack_core::schema::attribute_text`'s invisible-character module
//! doc): a variation selector is refused anywhere in a policy attribute,
//! and a refusal never writes a control or invisible character raw, in its
//! message or in its rendered code frame.

use crate::parse_schema;

const HEAD: &str = "auth SessionUser {\n  id Int\n  role String\n}\n\n\
                    type Summary {\n  total Int\n}\n\n\
                    model Doc {\n  id Int @id\n}\n\n";

/// Every policy attribute, with `{v}` where the role or action text goes.
const POLICIES: [&str; 6] = [
    "procedure p(n: Int): Summary\n  @allow(true)\n  @deny(hasRole(\"{v}\"))\n",
    "procedure p(n: Int): Summary\n  @allow(hasRole(\"{v}\"))\n",
    "model M {\n  id Int @id\n  @@deny(\"all\", auth().role == \"{v}\")\n}\n",
    "model M {\n  id Int @id\n  @@allow(\"read\", auth().role == \"{v}\")\n}\n",
    "view V from Doc {\n  id Int @id\n  @@server_sql(\"SELECT id FROM docs\")\n  \
     @@allow('read', auth().role == '{v}')\n}\n",
    "query q(n: Int): Summary\n  @@sql(\"SELECT $1::bigint AS total\")\n  @allow(true)\n  \
     @deny(hasRole(\"{v}\"))\n",
];

/// `"banné"` then U+FE0F renders as `"banné"`, yet a caller whose role is
/// `banné` passes the deny: after `é`, after CJK and after an emoji, a
/// selector in a policy is refused, pointing at the selector.
#[test]
fn a_variation_selector_in_a_policy_attribute_is_refused_after_any_character() {
    for role in [
        "banné\u{FE0F}",
        "管理者\u{FE0F}",
        "\u{2764}\u{FE0F}",
        "banné\u{E0100}",
        "\u{9F8D}\u{FE00}",
    ] {
        let selector = role.chars().last().unwrap();
        for policy in POLICIES {
            let body = policy.replace("{v}", role);
            let source = format!("{HEAD}{body}");
            let error = parse_schema(&source).expect_err(&body);
            assert_eq!(&source[error.span()], selector.to_string(), "{body:?}");
            assert!(
                error
                    .message()
                    .contains(&format!("U+{:04X}", selector as u32)),
                "{body:?}: {}",
                error.message()
            );
        }
        // The same text without the selector is an ordinary role.
        for policy in POLICIES {
            let body = policy.replace("{v}", role.trim_end_matches(selector));
            parse_schema(&format!("{HEAD}{body}"))
                .unwrap_or_else(|error| panic!("{body}: {error}"));
        }
    }
}

/// Outside a policy attribute the rule stays: an emoji keeps its selector
/// in a `@default` or a SQL body, and one after ASCII is still refused.
#[test]
fn outside_a_policy_a_selector_after_a_visible_character_is_allowed() {
    for text in ["\u{2764}\u{FE0F}", "banné\u{FE0F}", "\u{8FBA}\u{E0100}"] {
        for body in [
            format!("model M {{\n  id Int @id\n  name String @default(\"{text}\")\n}}\n"),
            format!(
                "query q(n: Int): Summary\n  @@sql(\"SELECT $1::bigint AS total -- {text}\")\n  \
                 @allow(true)\n"
            ),
        ] {
            parse_schema(&format!("{HEAD}{body}"))
                .unwrap_or_else(|error| panic!("{body}: {error}"));
        }
    }
    parse_schema(&format!(
        "{HEAD}model M {{\n  id Int @id\n  name String @default(\"n\u{FE0F}\")\n}}\n"
    ))
    .expect_err("n + U+FE0F is refused");
}

/// A refused NUL, BEL or ESC is quoted as `\u{…}` in the message, and the
/// rendered diagnostic (message and code frame) holds no raw control
/// character but the line breaks and the renderer's own colour codes.
#[test]
fn a_refusal_never_writes_a_control_character_raw() {
    for (ch, escaped) in [
        ('\u{0}', "\\u{0}"),
        ('\u{7}', "\\u{7}"),
        ('\u{1B}', "\\u{1B}"),
        ('\u{80}', "\\u{80}"),
        ('\u{200B}', "\\u{200B}"),
    ] {
        for body in [
            format!(
                "procedure p(n: Int): Summary\n  @allow(true)\n  @deny(hasRole(\"ban{ch}ned\"))\n"
            ),
            format!("model M {{\n  id Int @id\n  name String @default(\"ban{ch}ned\")\n}}\n"),
            format!("model M {{\n  id Int @id\n  @@de{ch}ny(\"all\", true)\n}}\n"),
        ] {
            let error = parse_schema(&format!("{HEAD}{body}")).expect_err(&body);
            let message = error.message();
            assert!(!message.contains(ch), "{message:?}");
            assert!(!message.chars().any(char::is_control), "{message:?}");
            if ch != '\u{1B}' {
                // ESC is refused by line, and that message quotes no text.
                assert!(message.contains(escaped), "{message:?}");
            }
            let rendered = strip_colour(&error.render());
            assert!(
                !rendered.chars().any(|c| c.is_control() && c != '\n'),
                "{rendered:?}"
            );
            assert!(!rendered.contains(ch), "{rendered:?}");
        }
    }
}

/// `rendered` without the `ESC [ … m` colour sequences the renderer
/// writes itself.
fn strip_colour(rendered: &str) -> String {
    let mut out = String::new();
    let mut chars = rendered.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1B}' && chars.peek() == Some(&'[') {
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}
