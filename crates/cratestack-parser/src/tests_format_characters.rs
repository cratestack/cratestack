//! Invisible characters in attribute text (GHSA-69g4-xvcm-vm2j,
//! maintainer decision 3; `parse::format_chars`). `"ban\u{34F}ned"`
//! displays as `"banned"` but is another string, so a deny comparing
//! against it never matches; inside a name the character makes another,
//! unread attribute. The set is Unicode's `Default_Ignorable_Code_Point`,
//! a variation selector not right after a visible non-ASCII character
//! (any one in a policy attribute, `tests_format_characters_policy`),
//! U+2800, U+1D159, the format controls U+FFF9–U+FFFB and U+13430–U+1343F, and
//! control characters that are not whitespace
//! (`cratestack_core::schema::attribute_text::invisible_character`).

use crate::parse_schema;

const HEAD: &str = "auth SessionUser {\n  id Int\n  role String\n}\n\n\
                    type Summary {\n  total Int\n}\n\n\
                    model Doc {\n  id Int @id\n}\n\n";

/// Each attribute position, with `{c}` where the character goes.
const POSITIONS: [&str; 7] = [
    "model M {\n  id Int @id\n  name String @default(\"ban{c}ned\")\n}\n",
    "model M {\n  id Int @id\n  @@deny(\"all\", auth().role == \"ban{c}ned\")\n}\n",
    "model M {\n  id Int @id\n  @@de{c}ny(\"all\", true)\n}\n",
    "view V from Doc {\n  id Int @id\n  @@server_sql(\"\"\"\n    SELECT id FROM docs \
     WHERE x = 'ban{c}ned'\n  \"\"\")\n}\n",
    "procedure p(n: Int): Summary\n  @allow(true)\n  @deny(hasRole(\"ban{c}ned\"))\n",
    "query q(n: Int): Summary\n  @@sql(\"SELECT $1::bigint AS total\")\n  @allow(true) \
     @deny(hasRole(\"ban{c}ned\"))\n",
    "query q(n: Int): Summary\n  @@sql(\"\"\"\n    SELECT $1::bigint AS total -- ban{c}ned\n  \
     \"\"\")\n  @allow(true)\n",
];

/// The characters the earlier `Cf` rule let through (each measured letting
/// a banned caller through a `@deny` over HTTP), U+2800, U+1D159, the zero-width
/// ones it already refused, the format controls
/// `Default_Ignorable_Code_Point` subtracts by name (U+FFF9–U+FFFB,
/// U+13430–U+1343F), and control characters that are not whitespace
/// (NUL, BEL, U+0080; ESC is refused on any line, `tests_hidden_breaks`).
/// Every one follows the ASCII `n`/`e` of the position, so the variation
/// selectors are refused too.
const INVISIBLE: [char; 32] = [
    '\u{034F}',
    '\u{3164}',
    '\u{115F}',
    '\u{1160}',
    '\u{FFA0}',
    '\u{180B}',
    '\u{180E}',
    '\u{180F}',
    '\u{17B4}',
    '\u{17B5}',
    '\u{FE0F}',
    '\u{FE00}',
    '\u{E0100}',
    '\u{2800}',
    '\u{1D159}',
    '\u{200B}',
    '\u{200C}',
    '\u{200D}',
    '\u{2060}',
    '\u{FEFF}',
    '\u{AD}',
    '\u{E0041}',
    '\u{2065}',
    '\u{061C}',
    '\u{1D173}',
    '\u{FFF9}',
    '\u{FFFB}',
    '\u{13430}',
    '\u{1343F}',
    '\u{00}',
    '\u{07}',
    '\u{80}',
];

#[test]
fn an_invisible_character_in_any_attribute_is_refused_with_its_position() {
    for ch in INVISIBLE {
        let code = format!("U+{:04X}", ch as u32);
        for position in POSITIONS {
            for newline in ["\n", "\r\n"] {
                let body = position.replace("{c}", &ch.to_string());
                let source = format!("{HEAD}{body}").replace('\n', newline);
                let error = parse_schema(&source).expect_err(&body);
                let message = error.to_string();
                assert!(message.contains(&code), "{body:?}: {message}");
                // The span is the character itself, and the line and column
                // named are where it stands.
                assert_eq!(&source[error.span()], ch.to_string(), "{body:?}");
                let line_start = source[..error.span().start]
                    .rfind('\n')
                    .map_or(0, |i| i + 1);
                let column = source[line_start..error.span().start].chars().count() + 1;
                let line = source[..error.span().start].matches('\n').count() + 1;
                assert_eq!(error.line(), line, "{body:?}");
                assert!(
                    message.contains(&format!("at line {line}, column {column}")),
                    "{body:?}: {message}"
                );
            }
        }
    }
}

/// The HTTP probe that got a 200 under the `Cf` rule is refused at parse
/// time.
#[test]
fn the_measured_deny_bypass_is_refused_at_parse_time() {
    let error = parse_schema(&format!(
        "{HEAD}procedure p(n: Int): Summary\n  @allow(true)\n  \
         @deny(hasRole(\"ban\u{034F}ned\"))\n"
    ))
    .expect_err("U+034F in a deny literal is refused");
    assert!(
        error
            .to_string()
            .contains("U+034F (combining grapheme joiner) at line"),
        "{error}"
    );
}

/// Visible non-ASCII text stays allowed in every position — the scripts
/// below and the visible `Cf` characters the earlier rule refused — as
/// does an emoji with its presentation selector outside a policy
/// attribute; an invisible character in a trailing comment is not
/// attribute text.
#[test]
fn visible_non_ascii_text_and_comments_are_not_affected() {
    for text in [
        "café",
        "中文",
        "Ærø",
        "ß😀",
        "한국어",
        "שלום",
        "مرحبا",
        "\u{2764}\u{FE0F}",
        "\u{0600}\u{0601}\u{0602}\u{0603}\u{0604}\u{0605}١",
        "\u{06DD}\u{08E2}\u{110BD}",
    ] {
        for position in POSITIONS {
            if position.contains("@@de{c}ny") {
                continue;
            }
            let policy = position.contains("@deny") || position.contains("@@deny");
            if policy && text.contains('\u{FE0F}') {
                continue;
            }
            let body = position.replace("ban{c}ned", text);
            parse_schema(&format!("{HEAD}{body}"))
                .unwrap_or_else(|error| panic!("{body}: {error}"));
        }
    }
    parse_schema(&format!(
        "{HEAD}procedure p(n: Int): Summary\n  @allow(true) // zero\u{200B}width\n  \
         // doc\u{200D}comment\n  @deny(hasRole(\"banned\"))\n"
    ))
    .unwrap_or_else(|error| panic!("{error}"));
}

/// A variation selector after an ASCII letter changes nothing on screen.
#[test]
fn a_variation_selector_after_ascii_is_refused() {
    let error = parse_schema(&format!(
        "{HEAD}model M {{\n  id Int @id\n  name String @default(\"n\u{FE0F}\")\n}}\n"
    ))
    .expect_err("n + U+FE0F is refused");
    assert!(
        error.to_string().contains("U+FE0F (a variation selector"),
        "{error}"
    );
}

/// Zero-width joiners stay refused (maintainer decision), so an emoji
/// joined with one is refused, even inside a string.
#[test]
fn an_emoji_joined_with_a_zero_width_joiner_is_refused() {
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
    let error = parse_schema(&format!(
        "{HEAD}model M {{\n  id Int @id\n  name String @default(\"{family}\")\n}}\n"
    ))
    .expect_err("a ZWJ sequence is refused");
    assert!(
        error.to_string().contains("U+200D (zero width joiner)"),
        "{error}"
    );
}
